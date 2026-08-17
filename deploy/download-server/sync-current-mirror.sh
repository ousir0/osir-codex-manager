#!/usr/bin/env bash
set -euo pipefail

# Seed the VPS with the currently published mirror. Run as root on the server
# after bootstrap.sh. Re-running it refreshes metadata and overwrites the
# latest artifacts; failed downloads do not replace existing files.

BASE="${SOURCE_BASE_URL:-https://app.osirclaw.com}"
PUBLIC_BASE="${PUBLIC_BASE_URL:-https://app.osirclaw.com}"
ROOT="${DOWNLOAD_ROOT:-/srv/osir}"
TMP="$(mktemp -d)"
trap 'rm -rf "${TMP}"' EXIT

mkdir -p "${ROOT}/latest" "${ROOT}/manager/latest" "${TMP}/latest" "${TMP}/manager/latest"

download() {
    local path="$1"
    local out="$2"
    curl --fail --location --retry 3 --retry-delay 2 --continue-at - \
        "${BASE}${path}" -o "${TMP}/${out}"
    install -m 0644 "${TMP}/${out}" "${ROOT}/${out}"
}

for path in manifest checksums appcast.xml appcast-x64.xml win-x64 win-arm64 mac-arm64 mac-intel; do
    download "/latest/${path}" "latest/${path}"
done

# /latest/win is the legacy x64 endpoint. A hard link avoids storing the same
# roughly 670 MB MSIX twice while preserving the existing client contract.
ln -f "${ROOT}/latest/win-x64" "${ROOT}/latest/win"

# Public manager installer links used by the website.
for name in \
    OSIRCodexManager_aarch64.dmg \
    OSIRCodexManager_x86_64.dmg \
    OSIRCodexManager_x64-setup.exe \
    OSIRCodexManager_arm64-setup.exe; do
    download "/manager/latest/${name}" "manager/latest/${name}"
done

# Self-updater metadata and its signed payloads. The signature covers bytes,
# not URLs, so replacing the host keeps updater verification valid.
latest_json="${TMP}/manager-latest.json"
curl --fail --location --retry 3 "${BASE}/manager/latest.json" -o "${latest_json}"
version="$(jq -er '.version' "${latest_json}")"
mkdir -p "${ROOT}/manager/${version}"

jq -r '.platforms[].url' "${latest_json}" | while IFS= read -r url; do
    file="${url##*/}"
    curl --fail --location --retry 3 --continue-at - "${url}" -o "${TMP}/${file}"
    install -m 0644 "${TMP}/${file}" "${ROOT}/manager/${version}/${file}"
done

jq --arg base "${PUBLIC_BASE}/manager/${version}" \
   '.platforms |= with_entries(.value.url = ($base + "/" + (.value.url | split("/") | last)))' \
   "${latest_json}" > "${TMP}/latest.json"
install -m 0644 "${TMP}/latest.json" "${ROOT}/manager/latest.json"

echo "Synced Codex latest artifacts and manager ${version}"
