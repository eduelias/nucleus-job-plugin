#!/usr/bin/env bash
# GDK custom build for dev.du7.nucleus-job-plugin.
#
# Invoked by `gdk component build` (see gdk-config.json). It:
#   1. cross-compiles the aarch64-linux release binary in a container,
#   2. stages the binary, setup.sh, and the sample-handlers zip into the GDK
#      artifacts folder, and
#   3. copies the recipe into the GDK recipes folder.
#
# GDK creates greengrass-build/{recipes,artifacts/<name>/<version>} beforehand and
# substitutes the recipe placeholders ({COMPONENT_VERSION}, BUCKET_NAME, ...) at
# build/publish time.
#
# Usage (by GDK): build-custom.sh <COMPONENT_NAME> <COMPONENT_VERSION>
set -euo pipefail

COMPONENT_NAME="${1:?component name required}"
COMPONENT_VERSION="${2:?component version required}"

# Repo root is the current working directory GDK runs the command from.
ROOT="$(pwd)"
ARTIFACTS_DIR="${ROOT}/greengrass-build/artifacts/${COMPONENT_NAME}/${COMPONENT_VERSION}"
RECIPES_DIR="${ROOT}/greengrass-build/recipes"
mkdir -p "${ARTIFACTS_DIR}" "${RECIPES_DIR}"

# Container runtime + image (override CONTAINER_ENGINE=docker if preferred).
ENGINE="${CONTAINER_ENGINE:-podman}"
IMAGE="${BUILD_IMAGE:-docker.io/library/rust:1-bookworm}"
TARGET_ARCH="${TARGET_ARCH:-arm64}"

echo "[build] cross-compiling aarch64 release binary (${ENGINE}, ${IMAGE})"
"${ENGINE}" run --rm --arch "${TARGET_ARCH}" \
  -v "${ROOT}":/src:Z -w /src -e CARGO_HOME=/src/.cargo-container \
  "${IMAGE}" \
  bash -c 'set -e; cargo build --release --locked --all-features; strip target/release/nucleus-job-plugin'
rm -rf "${ROOT}/.cargo-container"

echo "[build] staging artifacts -> ${ARTIFACTS_DIR}"
install -m 0755 "${ROOT}/target/release/nucleus-job-plugin" "${ARTIFACTS_DIR}/nucleus-job-plugin"
install -m 0755 "${ROOT}/greengrass/files/setup.sh" "${ARTIFACTS_DIR}/setup.sh"

# Bundle the sample job handlers as a zip (unarchived on the device).
STAGE="$(mktemp -d)"
mkdir -p "${STAGE}/handlers"
cp "${ROOT}"/greengrass/files/handlers/*.sh "${STAGE}/handlers/"
chmod +x "${STAGE}/handlers/"*.sh
( cd "${STAGE}" && zip -rq handlers.zip handlers )
mv "${STAGE}/handlers.zip" "${ARTIFACTS_DIR}/handlers.zip"
rm -rf "${STAGE}"

echo "[build] copying recipe -> ${RECIPES_DIR}"
cp "${ROOT}/recipe.json" "${RECIPES_DIR}/recipe.json"

echo "[build] done: $(ls -1 "${ARTIFACTS_DIR}")"
