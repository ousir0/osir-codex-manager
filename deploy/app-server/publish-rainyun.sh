#!/usr/bin/env bash
set -euo pipefail

SITE_DIR="${1:-}"
ARTIFACT_DIR="${2:-}"
RELEASE_ID="${3:-}"
SSH_TARGET="${OSIR_MANAGER_SSH_TARGET:-root@100.82.197.6}"
REMOTE_ROOT="/var/www/osir-codex-manager"
EXPECTED_SSH_TARGET="root@100.82.197.6"

if [[ -z "${SITE_DIR}" || -z "${ARTIFACT_DIR}" || -z "${RELEASE_ID}" ]]; then
  echo "usage: OSIR_MANAGER_SSH_TARGET=host $0 <site-dir> <artifact-dir> <release-id>" >&2
  exit 2
fi
if [[ "${SSH_TARGET}" != "${EXPECTED_SSH_TARGET}" ]]; then
  echo "refusing non-Tailscale Rainyun target: ${SSH_TARGET}" >&2
  echo "expected: ${EXPECTED_SSH_TARGET}" >&2
  exit 2
fi
if [[ ! "${RELEASE_ID}" =~ ^[0-9A-Za-z._-]+$ ]]; then
  echo "invalid release id: ${RELEASE_ID}" >&2
  exit 2
fi
[[ -d "${SITE_DIR}" ]] || { echo "site directory not found" >&2; exit 2; }
[[ -d "${ARTIFACT_DIR}" ]] || { echo "artifact directory not found" >&2; exit 2; }
[[ -f "${SITE_DIR}/index.html" ]] || { echo "site index.html missing" >&2; exit 2; }
[[ -f "${ARTIFACT_DIR}/manager/latest.json" ]] || { echo "manager latest.json missing" >&2; exit 2; }
[[ -f "${ARTIFACT_DIR}/.release-id" ]] || { echo "artifact release marker missing" >&2; exit 2; }
[[ "$(tr -d '\r\n' < "${ARTIFACT_DIR}/.release-id")" == "${RELEASE_ID}" ]] || { echo "artifact release marker does not match ${RELEASE_ID}" >&2; exit 2; }

REMOTE_TMP="${REMOTE_ROOT}/.${RELEASE_ID}.uploading"
REMOTE_RELEASE="${REMOTE_ROOT}/releases/${RELEASE_ID}"

REMOTE_STATE="$(ssh -o BatchMode=yes "${SSH_TARGET}" "set -eu; \
  if test -e '${REMOTE_RELEASE}'; then \
    test -f '${REMOTE_RELEASE}/.release-id'; \
    test "\$(tr -d '\\r\\n' < '${REMOTE_RELEASE}/.release-id')" = '${RELEASE_ID}'; \
    test "\$(readlink -f '${REMOTE_ROOT}/current')" = '${REMOTE_RELEASE}'; \
    printf 'current'; \
    exit 0; \
  fi; \
  test ! -e '${REMOTE_TMP}'; \
  mkdir -p '${REMOTE_TMP}/site' '${REMOTE_TMP}/manager' '${REMOTE_TMP}/latest' '${REMOTE_TMP}/skins'; \
  printf 'stage'")"
if [[ "${REMOTE_STATE}" == "current" ]]; then
  echo "release ${RELEASE_ID} is already published and current; nothing to do"
  exit 0
fi
tar -C "${SITE_DIR}" -czf - . | ssh -o BatchMode=yes "${SSH_TARGET}" "tar -xzf - -C '${REMOTE_TMP}/site'"
tar -C "${ARTIFACT_DIR}" -czf - . | ssh -o BatchMode=yes "${SSH_TARGET}" "tar -xzf - -C '${REMOTE_TMP}'"

ssh -o BatchMode=yes "${SSH_TARGET}" "set -eu; \
  test -f '${REMOTE_TMP}/site/index.html'; \
  test -f '${REMOTE_TMP}/manager/latest.json'; \
  test -f '${REMOTE_TMP}/manager/latest/CodexManager_aarch64.dmg'; \
  test -f '${REMOTE_TMP}/manager/latest/CodexManager_x86_64.dmg'; \
  test -f '${REMOTE_TMP}/manager/latest/CodexManager_${RELEASE_ID}_x64-setup.exe'; \
  test -f '${REMOTE_TMP}/manager/latest/CodexManager_${RELEASE_ID}_arm64-setup.exe'; \
  mkdir -p '${REMOTE_ROOT}/releases'; \
  mv '${REMOTE_TMP}' '${REMOTE_RELEASE}'; \
  find '${REMOTE_RELEASE}' -type d -exec chmod 0755 {} +; \
  find '${REMOTE_RELEASE}' -type f -exec chmod 0644 {} +; \
  ln -s '${REMOTE_RELEASE}' '${REMOTE_ROOT}/.current-${RELEASE_ID}'; \
  mv -Tf '${REMOTE_ROOT}/.current-${RELEASE_ID}' '${REMOTE_ROOT}/current'"

echo "published ${RELEASE_ID} to ${SSH_TARGET}:${REMOTE_RELEASE}"
