#!/usr/bin/env bash
# Install step (privileged) for the nucleus-job-plugin generic Greengrass component.
#
# Usage: setup.sh <artifacts_path> <handler_dir> [install_sample_handlers] [handlers_src_dir]
#
# * installs the native binary to /usr/local/bin
# * creates the allow-list handler directory owned by the component run-user
# * optionally installs the bundled AWS sample job handlers (for managed templates)
#
# The binary must be shipped alongside this script as an artifact named
# `nucleus-job-plugin` (built for the target architecture). The sample handlers,
# when installed, come from <handlers_src_dir> (default: `handlers/` next to this
# script) — override it when the handlers ship as a separate (unarchived) artifact.
set -euo pipefail

ARTIFACTS_PATH="${1:?artifacts path required}"
HANDLER_DIR="${2:-/var/lib/nucleus-job-plugin/handlers}"
INSTALL_SAMPLE_HANDLERS="${3:-1}"
HANDLERS_SRC="${4:-${ARTIFACTS_PATH}/handlers}"

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

if [ "${INSTALL_SAMPLE_HANDLERS}" = "1" ] && [ -d "${HANDLERS_SRC}" ]; then
  echo "[setup] installing AWS sample job handlers from ${HANDLERS_SRC} into ${HANDLER_DIR}"
  for h in "${HANDLERS_SRC}"/*.sh; do
    [ -e "$h" ] || continue
    install -m 0750 -o "${RUN_USER}" -g "${RUN_GROUP}" "$h" "${HANDLER_DIR}/$(basename "$h")"
  done
fi

echo "[setup] done"
