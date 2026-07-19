#!/usr/bin/env bash
# End-to-end demo for nucleus-job-plugin.
#
# Brings up the AWS IoT Greengrass nucleus in Docker (auto-provisioned from your
# AWS credentials), deploys nucleus-job-plugin as a LOCAL component via the
# in-container Greengrass CLI (no S3, no cloud component version), then creates a
# safe AWS-Run-Command IoT Job and waits for it to reach SUCCEEDED.
#
# Requirements: docker, aws CLI v2, git, curl, tar. Linux/x86_64 host (or Docker
# able to run linux/amd64 images — see README for Apple Silicon).
#
# ⚠️  Creates billable AWS resources (IoT thing, IAM role/alias, a Job). Run
#     ./teardown.sh afterwards. See README.md.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE"

COMPONENT_NAME="dev.du7.nucleus-job-plugin"
# Release to pull the prebuilt x86_64 binary from. Override with PLUGIN_VERSION.
PLUGIN_VERSION="${PLUGIN_VERSION:-0.1.2}"
GG_IMAGE="greengrass-nucleus:demo"
CONTAINER="njp-greengrass-demo"
STATE_FILE="$HERE/.demo-state"

log()  { printf '\033[0;32m[demo]\033[0m %s\n' "$*"; }
warn() { printf '\033[0;33m[demo]\033[0m %s\n' "$*"; }
die()  { printf '\033[0;31m[demo] ERROR:\033[0m %s\n' "$*" >&2; exit 1; }

# ---------------------------------------------------------------------------
# 1. Preflight
# ---------------------------------------------------------------------------
command -v docker >/dev/null || die "docker not found"
command -v aws    >/dev/null || die "aws CLI not found"
command -v curl   >/dev/null || die "curl not found"
docker compose version >/dev/null 2>&1 || die "docker compose plugin not found"

[ -f .env ] || die "Missing .env — copy .env.example to .env and set AWS_REGION."
[ -f greengrass-v2-credentials/credentials ] || \
  die "Missing greengrass-v2-credentials/credentials — copy the .example and add your keys."

# shellcheck disable=SC1091
set -a; . ./.env; set +a
: "${AWS_REGION:?set AWS_REGION in .env}"
export AWS_DEFAULT_REGION="$AWS_REGION"

# Give ephemeral resources a unique suffix and persist it for teardown.
if [ -f "$STATE_FILE" ]; then
  # shellcheck disable=SC1090
  . "$STATE_FILE"
  log "Resuming demo run: THING_NAME=$THING_NAME"
else
  SUFFIX="$(LC_ALL=C tr -dc 'a-z0-9' </dev/urandom | head -c6)"
  THING_NAME="${THING_NAME/REPLACE/$SUFFIX}"
  THING_GROUP_NAME="${THING_GROUP_NAME/REPLACE/$SUFFIX}"
  cat > "$STATE_FILE" <<EOF
THING_NAME=$THING_NAME
THING_GROUP_NAME=$THING_GROUP_NAME
AWS_REGION=$AWS_REGION
EOF
  log "Demo thing: $THING_NAME  group: $THING_GROUP_NAME  region: $AWS_REGION"
fi

# Rewrite the resolved names into .env so the container provisions them.
tmp_env="$(mktemp)"
sed -e "s/^THING_NAME=.*/THING_NAME=$THING_NAME/" \
    -e "s/^THING_GROUP_NAME=.*/THING_GROUP_NAME=$THING_GROUP_NAME/" .env > "$tmp_env"
mv "$tmp_env" .env

# ---------------------------------------------------------------------------
# 2. Build the official Greengrass image (AWS ships a Dockerfile, not an image)
# ---------------------------------------------------------------------------
if ! docker image inspect "$GG_IMAGE" >/dev/null 2>&1; then
  log "Building the Greengrass nucleus image from the official AWS Dockerfile..."
  BUILD_DIR="$(mktemp -d)"
  git clone --depth 1 https://github.com/aws-greengrass/aws-greengrass-docker.git "$BUILD_DIR" \
    || die "failed to clone aws-greengrass-docker"
  # The AWS Dockerfile downloads the nucleus at build time.
  docker build --platform linux/amd64 -t "$GG_IMAGE" "$BUILD_DIR" \
    || die "failed to build Greengrass image (see aws-greengrass-docker README)"
  rm -rf "$BUILD_DIR"
else
  log "Using existing image $GG_IMAGE"
fi

# ---------------------------------------------------------------------------
# 3. Start the nucleus (auto-provision) and wait until HEALTHY
# ---------------------------------------------------------------------------
log "Starting the Greengrass nucleus (this provisions AWS resources)..."
docker compose up -d

log "Waiting for the core device to become HEALTHY (up to ~5 min)..."
healthy=""
for _ in $(seq 1 60); do
  status="$(aws greengrassv2 get-core-device \
    --core-device-thing-name "$THING_NAME" \
    --query coreDeviceStatus --output text 2>/dev/null || true)"
  [ -n "$status" ] && log "  core device status: $status"
  if [ "$status" = "HEALTHY" ]; then healthy=1; break; fi
  sleep 5
done
[ -n "$healthy" ] || die "core device did not become HEALTHY — check: docker logs $CONTAINER"

# ---------------------------------------------------------------------------
# 4. Stage + deploy the plugin as a LOCAL component (Greengrass CLI, no S3)
# ---------------------------------------------------------------------------
log "Downloading nucleus-job-plugin $PLUGIN_VERSION (x86_64) release artifact..."
DL="$(mktemp -d)"
TARBALL="nucleus-job-plugin-${PLUGIN_VERSION}-x86_64-linux.tar.gz"
URL="https://github.com/eduelias/nucleus-job-plugin/releases/download/v${PLUGIN_VERSION}/${TARBALL}"
curl -fSL "$URL" -o "$DL/$TARBALL" \
  || die "could not download $URL (does a v$PLUGIN_VERSION release with an x86_64 asset exist?)"
tar -xzf "$DL/$TARBALL" -C "$DL"

# Build a local recipe + artifact layout for `greengrass-cli deployment create`.
#   recipeDir/   -> recipe file
#   artifactDir/<component>/<version>/  -> binary, setup.sh, handlers.zip
RECIPE_DIR="$DL/recipes"
ARTIFACT_DIR="$DL/artifacts/$COMPONENT_NAME/$PLUGIN_VERSION"
mkdir -p "$RECIPE_DIR" "$ARTIFACT_DIR"
cp "$DL/nucleus-job-plugin" "$DL/setup.sh" "$DL/handlers.zip" "$ARTIFACT_DIR/"

# The release recipe.json is templated for S3 URIs; for a local deployment we
# strip the Artifacts list (files are provided via --artifactDir) and pin the
# version. Uses python3 (present in the AWS Greengrass image and most hosts).
python3 - "$DL/recipe.json" "$RECIPE_DIR/${COMPONENT_NAME}-${PLUGIN_VERSION}.json" \
         "$COMPONENT_NAME" "$PLUGIN_VERSION" <<'PY'
import json, sys
src, dst, name, ver = sys.argv[1:5]
with open(src) as f:
    r = json.load(f)
r["ComponentName"] = name
r["ComponentVersion"] = ver
# Local artifacts are supplied via --artifactDir, so drop remote Artifact URIs.
for m in r.get("Manifests", []):
    m.pop("Artifacts", None)
with open(dst, "w") as f:
    json.dump(r, f, indent=2)
print("wrote", dst)
PY

# Copy the staged layout into the container and run a local deployment.
log "Deploying $COMPONENT_NAME $PLUGIN_VERSION as a local component..."
docker exec "$CONTAINER" mkdir -p /tmp/njp-demo
docker cp "$RECIPE_DIR"   "$CONTAINER:/tmp/njp-demo/recipes"
docker cp "$DL/artifacts" "$CONTAINER:/tmp/njp-demo/artifacts"
docker exec "$CONTAINER" /greengrass/v2/bin/greengrass-cli deployment create \
  --recipeDir /tmp/njp-demo/recipes \
  --artifactDir /tmp/njp-demo/artifacts \
  --merge "${COMPONENT_NAME}=${PLUGIN_VERSION}" \
  || die "local deployment failed"
rm -rf "$DL"

log "Waiting for $COMPONENT_NAME to reach RUNNING..."
running=""
for _ in $(seq 1 40); do
  state="$(docker exec "$CONTAINER" /greengrass/v2/bin/greengrass-cli component list 2>/dev/null \
    | awk -v c="$COMPONENT_NAME" '$0 ~ c {found=1} found && /State/ {print $NF; exit}')"
  [ -n "$state" ] && log "  component state: $state"
  if [ "$state" = "RUNNING" ]; then running=1; break; fi
  sleep 3
done
[ -n "$running" ] || warn "component not RUNNING yet — check: docker exec $CONTAINER tail -100 /greengrass/v2/logs/${COMPONENT_NAME}.log"

# ---------------------------------------------------------------------------
# 5. Create a safe AWS-Run-Command IoT Job and wait for SUCCEEDED
# ---------------------------------------------------------------------------
JOB_ID="njp-demo-runcmd-$(date +%s)"
echo "JOB_ID=$JOB_ID" >> "$STATE_FILE"
THING_ARN="$(aws iot describe-thing --thing-name "$THING_NAME" --query thingArn --output text)"
TEMPLATE_ARN="arn:aws:iot:${AWS_REGION}::jobtemplate/AWS-Run-Command:1.0"

log "Creating IoT Job $JOB_ID (AWS-Run-Command: touch a marker file)..."
aws iot create-job \
  --job-id "$JOB_ID" \
  --targets "$THING_ARN" \
  --job-template-arn "$TEMPLATE_ARN" \
  --document-parameters command="touch,/tmp/njp-demo-ok" \
  --query jobId --output text >/dev/null \
  || die "create-job failed (is the AWS-Run-Command managed template available in $AWS_REGION?)"

log "Waiting for the job execution to reach SUCCEEDED..."
final=""
for _ in $(seq 1 40); do
  st="$(aws iot describe-job-execution --job-id "$JOB_ID" --thing-name "$THING_NAME" \
    --query 'execution.status' --output text 2>/dev/null || true)"
  [ -n "$st" ] && log "  job status: $st"
  case "$st" in
    SUCCEEDED) final=SUCCEEDED; break ;;
    FAILED|TIMED_OUT|REJECTED|CANCELED) final="$st"; break ;;
  esac
  sleep 3
done

echo
if [ "$final" = "SUCCEEDED" ]; then
  log "✅ SUCCESS — job $JOB_ID reached SUCCEEDED."
  docker exec "$CONTAINER" sh -c 'ls -la /tmp/njp-demo-ok 2>/dev/null && echo "marker file created by the handler"' || true
  echo
  log "Recent component log:"
  docker exec "$CONTAINER" tail -20 "/greengrass/v2/logs/${COMPONENT_NAME}.log" 2>/dev/null \
    | sed 's/\x1b\[[0-9;]*m//g' || true
else
  warn "Job ended in state: ${final:-<timeout>}."
  warn "Inspect: docker exec $CONTAINER tail -100 /greengrass/v2/logs/${COMPONENT_NAME}.log"
fi

echo
log "Done. Run ./teardown.sh to delete the container and the AWS resources this demo created."
