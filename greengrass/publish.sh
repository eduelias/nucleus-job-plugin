#!/usr/bin/env bash
# Publish the component with GDK to whichever account/region the caller is
# pointed at.
#
# `gdk-config.json` deliberately pins neither a region nor a concrete bucket, so
# that this repository is not tied to any one AWS account. This wrapper supplies
# both at publish time.
#
# Usage:
#   greengrass/publish.sh
#
# Environment:
#   AWS_PROFILE   AWS credentials (optional; falls back to the default chain)
#   AWS_REGION    Target region. Falls back to `aws configure get region`.
#   GDK_BUCKET    Optional. An existing bucket to upload artifacts to, used
#                 verbatim. When unset, GDK derives the bucket from the
#                 `publish.bucket` prefix in gdk-config.json as
#                 "<prefix>-<region>-<account>" and creates it if needed.
#
#                 Set this when your organisation keeps Greengrass artifacts in
#                 a bucket whose name GDK would not generate. The core device's
#                 token exchange role must grant s3:GetObject on whichever
#                 bucket is used, or deployments fail with 403 Access Denied.
#
# Examples:
#   AWS_REGION=us-east-1 greengrass/publish.sh
#   AWS_REGION=eu-west-1 GDK_BUCKET=my-ggv2-components greengrass/publish.sh
set -euo pipefail

AWS_REGION="${AWS_REGION:-$(aws configure get region 2>/dev/null || true)}"
: "${AWS_REGION:?AWS_REGION required (or set a default region in your AWS profile)}"

echo "[publish] region=${AWS_REGION}${GDK_BUCKET:+ bucket=${GDK_BUCKET}}"

if [ -n "${GDK_BUCKET:-}" ]; then
  # GDK uses --bucket verbatim; the config prefix instead gets
  # "-<region>-<account>" appended.
  gdk component publish --region "${AWS_REGION}" --bucket "${GDK_BUCKET}"
else
  gdk component publish --region "${AWS_REGION}"
fi
