#!/usr/bin/env bash
# Install step (privileged) for the nucleus-job-plugin generic Greengrass component.
#
# Usage: setup.sh <artifacts_path> <handler_dir>
#
# * installs the native binary to /usr/local/bin
# * creates the allow-list handler directory owned by the component run-user
#
# The binary must be shipped alongside this script as an artifact named
# `nucleus-job-plugin` (built for the target architecture).
set -euo pipefail

ARTIFACTS_PATH="${1:?artifacts path required}"
HANDLER_DIR="${2:-/var/lib/nucleus-job-plugin/handlers}"

# The Greengrass default component run-user/group. Override if your nucleus uses
# a different runWith user.
RUN_USER="${GG_RUN_USER:-ggc_user}"
RUN_GROUP="${GG_RUN_GROUP:-ggc_group}"

BIN_SRC="${ARTIFACTS_PATH}/nucleus-job-plugin"
BIN_DST="/usr/local/bin/nucleus-job-plugin"

echo "[setup] installing binary -> ${BIN_DST}"
install -m 0755 "${BIN_SRC}" "${BIN_DST}"

echo "[setup] creating handler allow-list dir -> ${HANDLER_DIR}"
mkdir -p "${HANDLER_DIR}"
# Owned by the run-user; not world-writable so job documents can't drop handlers.
chown "${RUN_USER}:${RUN_GROUP}" "${HANDLER_DIR}"
chmod 0750 "${HANDLER_DIR}"

echo "[setup] done"
