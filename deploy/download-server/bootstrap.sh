#!/usr/bin/env bash
set -euo pipefail

# Run this script as root on the new Debian 12 server after DNS is pointed at
# the server. Caddy will obtain and renew the certificate for codexapp.awai.cc.

DOMAIN="${1:-codexapp.awai.cc}"
ROOT="/srv/awai"

if [[ "${EUID}" -ne 0 ]]; then
    echo "Run as root: sudo bash bootstrap.sh"
    exit 1
fi

export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y ca-certificates curl caddy jq rsync

install -d -m 0755 "${ROOT}/site" "${ROOT}/latest" "${ROOT}/manager"
install -d -m 0755 /etc/caddy

cat > /etc/caddy/Caddyfile <<EOF
$(sed "s/codexapp\.awai\.cc/${DOMAIN}/g" "$(dirname "$0")/Caddyfile")
EOF

caddy validate --config /etc/caddy/Caddyfile
systemctl enable caddy
systemctl restart caddy

echo "Ready: https://${DOMAIN}/"
echo "Upload the website build to ${ROOT}/site/"
echo "Upload Codex artifacts to ${ROOT}/latest/"
echo "Upload manager artifacts to ${ROOT}/manager/"
