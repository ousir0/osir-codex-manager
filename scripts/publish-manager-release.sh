#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TAG="${1:-}"
REPOSITORY="${GITHUB_REPOSITORY:-ousir0/osir-codex-manager}"

if [[ ! "${TAG}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$ ]]; then
  echo "usage: $0 vX.Y.Z" >&2
  exit 2
fi
command -v gh >/dev/null || { echo "gh CLI is required" >&2; exit 2; }
command -v sha256sum >/dev/null || { echo "sha256sum is required" >&2; exit 2; }

VERSION="${TAG#v}"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/codex-manager-release.XXXXXX")"
trap 'rm -rf "${WORK}"' EXIT
DOWNLOAD="${WORK}/download"
ARTIFACT="${WORK}/artifact"
mkdir -p "${DOWNLOAD}" "${ARTIFACT}/manager/${VERSION}" "${ARTIFACT}/manager/latest"

read -r IS_DRAFT IS_IMMUTABLE < <(gh release view "${TAG}" --repo "${REPOSITORY}" --json isDraft,isImmutable --jq '.isDraft, .isImmutable' | paste -sd ' ' -)
[[ "${IS_DRAFT}" == "false" && "${IS_IMMUTABLE}" == "true" ]] || { echo "release must be published and immutable" >&2; exit 1; }
gh release download "${TAG}" --repo "${REPOSITORY}" --dir "${DOWNLOAD}" --clobber
[[ -s "${DOWNLOAD}/latest.json" ]] || { echo "latest.json missing" >&2; exit 1; }
[[ -s "${DOWNLOAD}/SHA256SUMS" ]] || { echo "SHA256SUMS missing" >&2; exit 1; }
(cd "${DOWNLOAD}" && sha256sum -c SHA256SUMS)

for asset in "${DOWNLOAD}"/*; do
  name="$(basename "${asset}")"
  case "${name}" in
    latest.json|SHA256SUMS) continue ;;
  esac
  [[ -s "${asset}" ]] || { echo "empty release asset: ${name}" >&2; exit 1; }
  cp "${asset}" "${ARTIFACT}/manager/${VERSION}/${name}"
  cp "${asset}" "${ARTIFACT}/manager/latest/${name}"
done
for required in "CodexManager_aarch64.dmg" "CodexManager_x86_64.dmg" "CodexManager_${VERSION}_x64-setup.exe" "CodexManager_${VERSION}_arm64-setup.exe"; do
  [[ -s "${ARTIFACT}/manager/${VERSION}/${required}" ]] || { echo "required asset missing: ${required}" >&2; exit 1; }
done
cp "${DOWNLOAD}/latest.json" "${ARTIFACT}/manager/latest.json"
cp "${DOWNLOAD}/SHA256SUMS" "${ARTIFACT}/manager/${VERSION}/SHA256SUMS"
cp "${DOWNLOAD}/SHA256SUMS" "${ARTIFACT}/manager/latest/SHA256SUMS"
printf '%s\n' "${VERSION}" > "${ARTIFACT}/.release-id"

exec "${ROOT}/deploy/app-server/publish-rainyun.sh" "" "${ARTIFACT}" "${VERSION}"
