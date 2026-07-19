#!/usr/bin/env bash
# Tear down the nucleus-job-plugin demo: stop the container and delete the AWS
# resources created by run-demo.sh (IoT Job, thing + cert/policy, thing group).
# The token-exchange IAM role/alias are SHARED-named and are left in place by
# default (delete manually if this was a throwaway account).
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE"
STATE_FILE="$HERE/.demo-state"
CONTAINER="njp-greengrass-demo"

log()  { printf '\033[0;32m[teardown]\033[0m %s\n' "$*"; }
warn() { printf '\033[0;33m[teardown]\033[0m %s\n' "$*"; }

[ -f "$STATE_FILE" ] || { warn "No .demo-state found; nothing tracked to delete."; }
# shellcheck disable=SC1090
[ -f "$STATE_FILE" ] && . "$STATE_FILE"
: "${AWS_REGION:=${AWS_REGION:-}}"
[ -n "${AWS_REGION:-}" ] && export AWS_DEFAULT_REGION="$AWS_REGION"

log "Stopping and removing the Greengrass container + volumes..."
docker compose down -v 2>/dev/null || docker rm -f "$CONTAINER" 2>/dev/null || true

if [ -z "${THING_NAME:-}" ]; then
  warn "No THING_NAME tracked; skipping AWS cleanup."
  exit 0
fi

# 1. Delete the IoT Job (force cancel + delete).
if [ -n "${JOB_ID:-}" ]; then
  log "Deleting IoT Job $JOB_ID..."
  aws iot cancel-job --job-id "$JOB_ID" --force 2>/dev/null || true
  sleep 3
  aws iot delete-job --job-id "$JOB_ID" --force 2>/dev/null || true
fi

# 2. Detach + delete the thing's certificate(s) and policies, then the thing.
log "Cleaning up thing $THING_NAME (certificates, policies)..."
for principal in $(aws iot list-thing-principals --thing-name "$THING_NAME" \
    --query 'principals[]' --output text 2>/dev/null || true); do
  cert_id="${principal##*/}"
  # Detach policies from the cert.
  for pol in $(aws iot list-attached-policies --target "$principal" \
      --query 'policies[].policyName' --output text 2>/dev/null || true); do
    aws iot detach-policy --policy-name "$pol" --target "$principal" 2>/dev/null || true
  done
  aws iot detach-thing-principal --thing-name "$THING_NAME" --principal "$principal" 2>/dev/null || true
  aws iot update-certificate --certificate-id "$cert_id" --new-status INACTIVE 2>/dev/null || true
  aws iot delete-certificate --certificate-id "$cert_id" --force-delete 2>/dev/null || true
done
aws iot delete-thing --thing-name "$THING_NAME" 2>/dev/null || true

# 3. Delete the thing group.
if [ -n "${THING_GROUP_NAME:-}" ]; then
  log "Deleting thing group $THING_GROUP_NAME..."
  aws iot delete-thing-group --thing-group-name "$THING_GROUP_NAME" 2>/dev/null || true
fi

# 4. Delete the core device registration (if it lingers).
aws greengrassv2 delete-core-device --core-device-thing-name "$THING_NAME" 2>/dev/null || true

log "Removing local state + credentials copy."
rm -f "$STATE_FILE"
warn "Remove your credentials file when done: rm -f greengrass-v2-credentials/credentials"
warn "The token-exchange IAM role/alias (GreengrassV2TokenExchangeRole / ...Alias) were left in"
warn "place (they are reused across Greengrass devices). Delete them manually if unwanted."
log "Teardown complete."
