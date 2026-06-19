#!/usr/bin/env sh

set -eu

artifact_path="${1:?missing artifact path}"
output_path="${2:?missing output path}"

if [ -z "${GH_TOKEN:-}" ]; then
  echo "GH_TOKEN is required" >&2
  exit 1
fi

artifact_path="$(realpath "${artifact_path}")"
tmpdir="$(mktemp -d)"
trap 'rm -rf "${tmpdir}"' EXIT INT TERM

(
  cd "${tmpdir}"
  gh attestation download "${artifact_path}" -R "${GITHUB_REPOSITORY}"
)

attestation_bundle="$(find "${tmpdir}" -maxdepth 1 -type f -name 'sha256*.jsonl' | head -1)"
test -n "${attestation_bundle}" || {
  echo "failed to download attestation bundle" >&2
  exit 1
}

mkdir -p "$(dirname "${output_path}")"
mv "${attestation_bundle}" "${output_path}"
