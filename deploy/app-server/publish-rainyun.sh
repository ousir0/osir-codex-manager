#!/usr/bin/env bash
set -euo pipefail

SITE_DIR="${1:-}"
ARTIFACT_DIR="${2:-}"
RELEASE_ID="${3:-}"
SSH_TARGET="${OSIR_MANAGER_SSH_TARGET:-}"
REMOTE_ROOT="/var/www/osir-codex-manager"

if [[ -z "${SITE_DIR}" || -z "${ARTIFACT_DIR}" || -z "${RELEASE_ID}" ]]; then
  echo "usage: OSIR_MANAGER_SSH_TARGET=host $0 <site-dir> <artifact-dir> <release-id>" >&2
  exit 2
fi
if [[ -z "${SSH_TARGET}" ]]; then
  echo "OSIR_MANAGER_SSH_TARGET is required" >&2
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

REMOTE_TMP="${REMOTE_ROOT}/.${RELEASE_ID}.uploading"
REMOTE_RELEASE="${REMOTE_ROOT}/releases/${RELEASE_ID}"

ssh -o BatchMode=yes "${SSH_TARGET}" "set -eu; test ! -e '${REMOTE_RELEASE}'; test ! -e '${REMOTE_TMP}'; mkdir -p '${REMOTE_TMP}/site' '${REMOTE_TMP}/manager' '${REMOTE_TMP}/latest' '${REMOTE_TMP}/skins'"
tar -C "${SITE_DIR}" -czf - . | ssh -o BatchMode=yes "${SSH_TARGET}" "tar -xzf - -C '${REMOTE_TMP}/site'"
tar -C "${ARTIFACT_DIR}" -czf - . | ssh -o BatchMode=yes "${SSH_TARGET}" "tar -xzf - -C '${REMOTE_TMP}'"

ssh -o BatchMode=yes "${SSH_TARGET}" "set -eu; \
  test -f '${REMOTE_TMP}/site/index.html'; \
  test -f '${REMOTE_TMP}/manager/latest.json'; \
  mkdir -p '${REMOTE_ROOT}/releases'; \
  mv '${REMOTE_TMP}' '${REMOTE_RELEASE}'; \
  ln -s '${REMOTE_RELEASE}' '${REMOTE_ROOT}/.current-${RELEASE_ID}'; \
  mv -Tf '${REMOTE_ROOT}/.current-${RELEASE_ID}' '${REMOTE_ROOT}/current'"

echo "published ${RELEASE_ID} to ${SSH_TARGET}:${REMOTE_RELEASE}"
