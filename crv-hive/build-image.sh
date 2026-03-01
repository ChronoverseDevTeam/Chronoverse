#!/usr/bin/env bash
set -euo pipefail

# Usage:
#   ./build-image.sh [image-name]
#   ENGINE=podman ./build-image.sh [image-name]

IMAGE_NAME="${1:-crv-hive:latest}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
TARGET_TRIPLE="x86_64-unknown-linux-musl"
BINARY_PATH="${REPO_ROOT}/target/${TARGET_TRIPLE}/release/crv-hive"

detect_engine() {
  if [[ -n "${ENGINE:-}" ]]; then
    echo "${ENGINE}"
    return
  fi

  if command -v podman >/dev/null 2>&1; then
    echo "podman"
    return
  fi

  if command -v docker >/dev/null 2>&1; then
    echo "docker"
    return
  fi

  echo "No container engine found. Please install podman or docker." >&2
  exit 1
}

ENGINE_CMD="$(detect_engine)"

echo "==> Repo root: ${REPO_ROOT}"
echo "==> Build target: ${TARGET_TRIPLE}"
echo "==> Container engine: ${ENGINE_CMD}"

cd "${REPO_ROOT}"

echo "==> Ensuring rust target exists: ${TARGET_TRIPLE}"
rustup target add "${TARGET_TRIPLE}"

echo "==> Building release binary: crv-hive"
cargo build -p crv-hive --release --target "${TARGET_TRIPLE}"

if [[ ! -f "${BINARY_PATH}" ]]; then
  echo "Release binary not found: ${BINARY_PATH}" >&2
  exit 1
fi

echo "==> Building image: ${IMAGE_NAME}"
"${ENGINE_CMD}" build -f "${SCRIPT_DIR}/Dockerfile" -t "${IMAGE_NAME}" "${REPO_ROOT}"

echo "==> Done. Image built: ${IMAGE_NAME}"
