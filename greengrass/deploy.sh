#!/usr/bin/env bash
# Deploy a published component version to a thing or thing group, and wait for the
# deployment to complete. Complements GDK, which builds/publishes but does not
# deploy.
#
# A Greengrass deployment defines the COMPLETE component set for its target, so
# creating one naively removes every other component previously deployed to that
# same target. By default this script therefore reads the target's current
# deployment and merges this component into it, which is what the AWS console
# does when you revise a deployment. Set MERGE=0 to get the raw
# replace-everything behaviour.
#
# Components deployed to a *different* target (for example a thing group the
# device also belongs to) live in their own deployment and are never affected.
#
# Usage:
#   greengrass/deploy.sh <component-version> [target]
#
#   <target> may be a thing name, a thing group name (with TARGET_TYPE=thinggroup),
#   or a full IoT ARN. Defaults to $TARGET_THING.
#
# Environment:
#   AWS_PROFILE   AWS credentials (optional; falls back to the default chain)
#   AWS_REGION    Target region. Falls back to `aws configure get region`.
#   TARGET_THING  Default target if not passed as $2.
#   TARGET_TYPE   "thing" (default) or "thinggroup". Ignored if <target> is an ARN.
#   MERGE         "1" (default) preserves the target's existing components.
#                 "0" deploys only this component, removing any others.
#
# Examples:
#   AWS_REGION=us-east-1 greengrass/deploy.sh 0.1.7 MyGreengrassCoreThing
#   AWS_REGION=eu-west-1 TARGET_TYPE=thinggroup greengrass/deploy.sh 0.1.7 MyFleet
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Keep the component name in sync with gdk-config.json so forks that rename the
# component do not have to edit this script.
COMPONENT_NAME="$(python3 -c '
import json,sys
cfg=json.load(open(sys.argv[1]))["component"]
sys.stdout.write(next(iter(cfg)))
' "${REPO_ROOT}/gdk-config.json")"

VERSION="${1:?component version required (e.g. 0.1.7)}"
TARGET="${2:-${TARGET_THING:-}}"
: "${TARGET:?target thing or thing group required (arg 2 or TARGET_THING)}"
MERGE="${MERGE:-1}"

AWS_REGION="${AWS_REGION:-$(aws configure get region 2>/dev/null || true)}"
: "${AWS_REGION:?AWS_REGION required (or set a default region in your AWS profile)}"
export AWS_REGION

if [[ "${TARGET}" == arn:* ]]; then
  TARGET_ARN="${TARGET}"
else
  # Derive the account and partition from the caller so this works in aws,
  # aws-us-gov and aws-cn alike.
  read -r PARTITION ACCOUNT <<<"$(aws sts get-caller-identity \
    --query '[Arn,Account]' --output text | awk '{split($1,a,":"); print a[2], $2}')"
  : "${PARTITION:?could not determine AWS partition from caller identity}"
  : "${ACCOUNT:?could not determine AWS account from caller identity}"
  case "${TARGET_TYPE:-thing}" in
    thing)      TARGET_ARN="arn:${PARTITION}:iot:${AWS_REGION}:${ACCOUNT}:thing/${TARGET}" ;;
    thinggroup) TARGET_ARN="arn:${PARTITION}:iot:${AWS_REGION}:${ACCOUNT}:thinggroup/${TARGET}" ;;
    *) echo "[deploy] TARGET_TYPE must be 'thing' or 'thinggroup'" >&2; exit 2 ;;
  esac
fi

echo "[deploy] ${COMPONENT_NAME} ${VERSION} -> ${TARGET_ARN}"

# Start from the target's current component set so we revise rather than replace.
BASE_COMPONENTS='{}'
if [ "${MERGE}" = "1" ]; then
  LATEST_ID="$(aws greengrassv2 list-deployments \
    --target-arn "${TARGET_ARN}" --history-filter LATEST_ONLY \
    --query 'deployments[0].deploymentId' --output text 2>/dev/null || echo None)"
  if [ -n "${LATEST_ID}" ] && [ "${LATEST_ID}" != "None" ]; then
    BASE_COMPONENTS="$(aws greengrassv2 get-deployment \
      --deployment-id "${LATEST_ID}" --query components --output json 2>/dev/null || echo '{}')"
    [ "${BASE_COMPONENTS}" = "null" ] && BASE_COMPONENTS='{}'
    echo "[deploy] merging into existing deployment ${LATEST_ID}"
  else
    echo "[deploy] no existing deployment for this target; creating the first one"
  fi
else
  echo "[deploy] MERGE=0: deploying only ${COMPONENT_NAME} (other components on this target are removed)"
fi

COMPONENTS="$(python3 -c '
import json,sys
base=json.loads(sys.argv[1]) or {}
# Drop keys the API rejects on the way back in.
for spec in base.values():
    spec.pop("componentUpdatePolicy", None)
base[sys.argv[2]]={"componentVersion": sys.argv[3]}
sys.stdout.write(json.dumps(base))
' "${BASE_COMPONENTS}" "${COMPONENT_NAME}" "${VERSION}")"

SUMMARY="$(printf '%s' "${COMPONENTS}" | python3 -c '
import json,sys
d=json.load(sys.stdin)
print(", ".join(k+"="+v.get("componentVersion","?") for k,v in sorted(d.items())))
')"
echo "[deploy] components: ${SUMMARY}"

DEPLOY_ID="$(aws greengrassv2 create-deployment \
  --target-arn "${TARGET_ARN}" \
  --deployment-name "${COMPONENT_NAME}-${VERSION}" \
  --components "${COMPONENTS}" \
  --deployment-policies '{"failureHandlingPolicy":"DO_NOTHING","componentUpdatePolicy":{"action":"NOTIFY_COMPONENTS","timeoutInSeconds":60}}' \
  --query deploymentId --output text)"
echo "[deploy] deploymentId=${DEPLOY_ID}"

# Only a thing target has a single core device whose lifecycle state we can poll;
# for a thing group just wait on the deployment itself.
POLL_THING=""
if [[ "${TARGET_ARN}" == *":thing/"* ]]; then
  POLL_THING="${TARGET_ARN##*/}"
fi

echo "[deploy] waiting for the deployment to complete..."
for _ in $(seq 1 40); do
  DSTATUS="$(aws greengrassv2 get-deployment --deployment-id "${DEPLOY_ID}" --query deploymentStatus --output text)"
  LSTATE="None"
  if [ -n "${POLL_THING}" ]; then
    LSTATE="$(aws greengrassv2 list-installed-components \
      --core-device-thing-name "${POLL_THING}" \
      --query "installedComponents[?componentName=='${COMPONENT_NAME}'].lifecycleState | [0]" \
      --output text 2>/dev/null || echo None)"
  fi
  echo "  deployment=${DSTATUS} component=${LSTATE}"
  case "${DSTATUS}" in
    FAILED|CANCELED) echo "[deploy] deployment ${DSTATUS}"; exit 1 ;;
  esac
  case "${LSTATE}" in
    RUNNING|FINISHED) echo "[deploy] component ${LSTATE}"; exit 0 ;;
    BROKEN|ERRORED) echo "[deploy] component ${LSTATE}"; exit 1 ;;
  esac
  if [ "${DSTATUS}" = "COMPLETED" ] && [ -z "${POLL_THING}" ]; then
    echo "[deploy] deployment COMPLETED"; exit 0
  fi
  sleep 6
done
echo "[deploy] timed out (deployment ${DEPLOY_ID} may still be in progress)"
exit 1
