#!/usr/bin/env bash
set -euo pipefail

SITE_DIR="${1:-}"
ARTIFACT_DIR="${2:-}"
RELEASE_ID="${3:-}"
TARGET="${OSIR_MANAGER_SSH_TARGET:-root@100.82.197.6}"
REMOTE_ROOT="${OSIR_MANAGER_REMOTE_ROOT:-/var/www/osir-codex-manager}"

if [[ -z "${ARTIFACT_DIR}" || -z "${RELEASE_ID}" ]]; then
  echo "usage: $0 [site-dir] <artifact-dir> <release-id>" >&2
  exit 2
fi
[[ "${TARGET}" == root@100.82.197.6 ]] || { echo "refusing non-Tailscale Rainyun target" >&2; exit 2; }
[[ "${RELEASE_ID}" =~ ^[0-9A-Za-z._-]+$ ]] || { echo "invalid release id: ${RELEASE_ID}" >&2; exit 2; }
[[ -d "${ARTIFACT_DIR}" ]] || { echo "artifact directory not found" >&2; exit 2; }
[[ -f "${ARTIFACT_DIR}/manager/latest.json" ]] || { echo "manager latest.json missing" >&2; exit 2; }
[[ -f "${ARTIFACT_DIR}/.release-id" ]] || { echo "artifact release marker missing" >&2; exit 2; }
[[ "$(tr -d '\r\n' < "${ARTIFACT_DIR}/.release-id")" == "${RELEASE_ID}" ]] || { echo "artifact release marker mismatch" >&2; exit 2; }

REMOTE_RELEASE="${REMOTE_ROOT}/releases/${RELEASE_ID}"
REMOTE_TMP="${REMOTE_ROOT}/.${RELEASE_ID}.uploading"

if ssh -o BatchMode=yes "${TARGET}" test -e "${REMOTE_RELEASE}"; then
  current="$(ssh -o BatchMode=yes "${TARGET}" readlink -f "${REMOTE_ROOT}/current")"
  [[ "${current}" == "${REMOTE_RELEASE}" ]] || { echo "release exists but current points elsewhere" >&2; exit 1; }
  echo "release ${RELEASE_ID} is already current; nothing to do"
  exit 0
fi

ssh -o BatchMode=yes "${TARGET}" test ! -e "${REMOTE_TMP}"
current="$(ssh -o BatchMode=yes "${TARGET}" readlink -f "${REMOTE_ROOT}/current")"
ssh -o BatchMode=yes "${TARGET}" test -f "${current}/site/index.html"
ssh -o BatchMode=yes "${TARGET}" mkdir -p "${REMOTE_TMP}/manager" "${REMOTE_TMP}/latest" "${REMOTE_TMP}/skins"
ssh -o BatchMode=yes "${TARGET}" cp -a "${current}/site" "${REMOTE_TMP}/site"
tar -C "${ARTIFACT_DIR}" -czf - . | ssh -o BatchMode=yes "${TARGET}" "tar -xzf - -C '${REMOTE_TMP}'"

ssh -o BatchMode=yes "${TARGET}" test -f "${REMOTE_TMP}/site/index.html"
ssh -o BatchMode=yes "${TARGET}" test -f "${REMOTE_TMP}/manager/latest.json"
ssh -o BatchMode=yes "${TARGET}" test -f "${REMOTE_TMP}/manager/latest/CodexManager_aarch64.dmg"
ssh -o BatchMode=yes "${TARGET}" test -f "${REMOTE_TMP}/manager/latest/CodexManager_x86_64.dmg"
ssh -o BatchMode=yes "${TARGET}" test -f "${REMOTE_TMP}/manager/latest/CodexManager_${RELEASE_ID}_x64-setup.exe"
ssh -o BatchMode=yes "${TARGET}" test -f "${REMOTE_TMP}/manager/latest/CodexManager_${RELEASE_ID}_arm64-setup.exe"
ssh -o BatchMode=yes "${TARGET}" cp "${REMOTE_TMP}/manager/latest/CodexManager_${RELEASE_ID}_x64-setup.exe" "${REMOTE_TMP}/manager/latest/CodexManager_x64-setup.exe"
ssh -o BatchMode=yes "${TARGET}" cp "${REMOTE_TMP}/manager/latest/CodexManager_${RELEASE_ID}_x64-setup.exe.sig" "${REMOTE_TMP}/manager/latest/CodexManager_x64-setup.exe.sig"
ssh -o BatchMode=yes "${TARGET}" cp "${REMOTE_TMP}/manager/latest/CodexManager_${RELEASE_ID}_arm64-setup.exe" "${REMOTE_TMP}/manager/latest/CodexManager_arm64-setup.exe"
ssh -o BatchMode=yes "${TARGET}" cp "${REMOTE_TMP}/manager/latest/CodexManager_${RELEASE_ID}_arm64-setup.exe.sig" "${REMOTE_TMP}/manager/latest/CodexManager_arm64-setup.exe.sig"
ssh -o BatchMode=yes "${TARGET}" test -f "${REMOTE_TMP}/manager/${RELEASE_ID}/SHA256SUMS"
ssh -o BatchMode=yes "${TARGET}" find "${REMOTE_TMP}/manager/${RELEASE_ID}" -type f -name '*.sig' -print -quit | grep -q .
ssh -o BatchMode=yes "${TARGET}" mkdir -p "${REMOTE_ROOT}/releases"
ssh -o BatchMode=yes "${TARGET}" mv "${REMOTE_TMP}" "${REMOTE_RELEASE}"
ssh -o BatchMode=yes "${TARGET}" find "${REMOTE_RELEASE}" -type d -exec chmod 0755 '{}' +
ssh -o BatchMode=yes "${TARGET}" find "${REMOTE_RELEASE}" -type f -exec chmod 0644 '{}' +
printf '%s\n' "${RELEASE_ID}" | ssh -o BatchMode=yes "${TARGET}" tee "${REMOTE_RELEASE}/.release-id" >/dev/null
ssh -o BatchMode=yes "${TARGET}" ln -s "${REMOTE_RELEASE}" "${REMOTE_ROOT}/.current-${RELEASE_ID}"
ssh -o BatchMode=yes "${TARGET}" mv -Tf "${REMOTE_ROOT}/.current-${RELEASE_ID}" "${REMOTE_ROOT}/current"
echo "published ${RELEASE_ID} to ${TARGET}:${REMOTE_RELEASE}"
