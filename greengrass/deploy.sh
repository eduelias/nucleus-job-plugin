#!/usr/bin/env bash
# Deploy a published component version to a thing (or thing group) and wait for the
# deployment to complete. Complements GDK, which builds/publishes but does not
# deploy.
#
# Usage:
#   greengrass/deploy.sh <component-version> [thing-name]
#
# Environment:
#   AWS_PROFILE / AWS_REGION   AWS credentials + region (required)
#   TARGET_THING               default thing name if not passed as $2
#
# Examples:
#   AWS_PROFILE=pnl-apl-dev-elevated AWS_REGION=eu-west-1 \
#     greengrass/deploy.sh 0.1.5 fp_777007_000000007bac5a8e
set -euo pipefail

COMPONENT_NAME="dev.du7.nucleus-job-plugin"
VERSION="${1:?component version required (e.g. 0.1.5)}"
THING="${2:-${TARGET_THING:-}}"
: "${THING:?thing name required (arg 2 or TARGET_THING)}"
: "${AWS_REGION:?AWS_REGION required}"

ACCOUNT="$(aws sts get-caller-identity --query Account --output text)"
THING_ARN="arn:aws:iot:${AWS_REGION}:${ACCOUNT}:thing/${THING}"

echo "[deploy] ${COMPONENT_NAME} ${VERSION} -> ${THING}"
DEPLOY_ID="$(aws greengrassv2 create-deployment \
  --target-arn "${THING_ARN}" \
  --deployment-name "${COMPONENT_NAME}-${VERSION}" \
  --components "{\"${COMPONENT_NAME}\":{\"componentVersion\":\"${VERSION}\"}}" \
  --deployment-policies '{"failureHandlingPolicy":"DO_NOTHING","componentUpdatePolicy":{"action":"NOTIFY_COMPONENTS","timeoutInSeconds":60}}' \
  --query deploymentId --output text)"
echo "[deploy] deploymentId=${DEPLOY_ID}"

echo "[deploy] waiting for the component to reach RUNNING on the device..."
for _ in $(seq 1 40); do
  DSTATUS="$(aws greengrassv2 get-deployment --deployment-id "${DEPLOY_ID}" --query deploymentStatus --output text)"
  case "${DSTATUS}" in
    FAILED|CANCELED) echo "[deploy] deployment ${DSTATUS}"; exit 1 ;;
  esac
  LSTATE="$(aws greengrassv2 list-installed-components \
    --core-device-thing-name "${THING}" \
    --query "installedComponents[?componentName=='${COMPONENT_NAME}'].lifecycleState | [0]" \
    --output text 2>/dev/null || echo None)"
  echo "  deployment=${DSTATUS} component=${LSTATE}"
  case "${LSTATE}" in
    RUNNING|FINISHED) echo "[deploy] component ${LSTATE}"; exit 0 ;;
    BROKEN|ERRORED) echo "[deploy] component ${LSTATE}"; exit 1 ;;
  esac
  sleep 6
done
echo "[deploy] timed out (deployment ${DEPLOY_ID} may still be in progress)"
exit 1
