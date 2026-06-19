#!/usr/bin/env sh

set -eu

REPO_OWNER="herakles-now"
REPO_NAME="herakles-node-exporter"
BIN_NAME="herakles-node-exporter"
VERSION="latest"

usage() {
  cat <<'EOF'
Install herakles-node-exporter from GitHub release artifacts.

Usage:
  install.sh [options]

Options:
  --version <tag>      Install a specific release tag. Default: latest
  -h, --help           Show this help

This script requires root and runs `herakles-node-exporter install`
after downloading the release binary.

Examples:
  sudo sh install.sh
  sudo sh install.sh --version <version>
EOF
}

log() {
  printf '%s\n' "$*" >&2
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || {
    log "missing required command: $1"
    exit 1
  }
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --version)
      VERSION="${2:?missing value for --version}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      log "unknown argument: $1"
      usage
      exit 1
      ;;
  esac
done

need_cmd curl
need_cmd uname
need_cmd mktemp
need_cmd sha256sum

if [ "$(id -u)" -ne 0 ]; then
  log "root privileges are required"
  exit 1
fi

detect_arch() {
  case "$(uname -m)" in
    x86_64|amd64) printf 'x86_64\n' ;;
    aarch64|arm64) printf 'aarch64\n' ;;
    *)
      log "unsupported architecture: $(uname -m)"
      exit 1
      ;;
  esac
}

detect_libc() {
  if command -v ldd >/dev/null 2>&1; then
    ldd_output="$(ldd --version 2>&1 || true)"
    case "${ldd_output}" in
      *musl*) printf 'musl\n'; return ;;
      *glibc*|*GNU\ libc*|*GNU\ C\ Library*) printf 'gnu\n'; return ;;
    esac
  fi

  if getconf GNU_LIBC_VERSION >/dev/null 2>&1; then
    printf 'gnu\n'
    return
  fi

  printf 'gnu\n'
}

resolve_version() {
  if [ "${VERSION}" != "latest" ]; then
    printf '%s\n' "${VERSION}"
    return
  fi

  latest_json="$(curl -fsSL "https://api.github.com/repos/${REPO_OWNER}/${REPO_NAME}/releases/latest")"
  latest_tag="$(printf '%s\n' "${latest_json}" | sed -n 's/.*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' | head -n1)"
  if [ -z "${latest_tag}" ]; then
    log "failed to resolve latest release tag"
    exit 1
  fi
  printf '%s\n' "${latest_tag}"
}

ARCH="$(detect_arch)"
LIBC="$(detect_libc)"
TARGET="${ARCH}-linux-${LIBC}"
RESOLVED_VERSION="$(resolve_version)"
ASSET_URL="https://github.com/${REPO_OWNER}/${REPO_NAME}/releases/download/${RESOLVED_VERSION}/${BIN_NAME}-${TARGET}"
SHA_URL="https://github.com/${REPO_OWNER}/${REPO_NAME}/releases/download/${RESOLVED_VERSION}/${BIN_NAME}-${RESOLVED_VERSION}-${TARGET}.sha256"

tmpdir="$(mktemp -d)"
trap 'rm -rf "${tmpdir}"' EXIT INT TERM

bin_path="${tmpdir}/${BIN_NAME}"
sha_path="${tmpdir}/${BIN_NAME}.sha256"

log "Installing ${BIN_NAME} ${RESOLVED_VERSION} for ${TARGET}"
curl -fsSL "${ASSET_URL}" -o "${bin_path}"
curl -fsSL "${SHA_URL}" -o "${sha_path}"

expected_sha="$(awk '{print $1}' "${sha_path}")"
actual_sha="$(sha256sum "${bin_path}" | awk '{print $1}')"

if [ "${expected_sha}" != "${actual_sha}" ]; then
  log "checksum verification failed"
  log "expected: ${expected_sha}"
  log "actual:   ${actual_sha}"
  exit 1
fi

log "Running system installation"
"${bin_path}" install
