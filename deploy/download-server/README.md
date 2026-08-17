# `app.osirclaw.com` VPS

This is the low-cost single-server deployment for the first release. It keeps
the website and installer bytes on the VPS and serves them with Caddy. The
server's 20 Mbps port is the download ceiling; keep only the latest and one
previous release until a CDN or object storage is added.

## First boot

1. Point a Cloudflare DNS `A` record named `download` at the VPS public IPv4.
   Leave the proxy disabled while validating large downloads.
2. Copy this directory to the server and run:

   ```bash
   sudo bash bootstrap.sh
   sudo bash sync-current-mirror.sh
   ```

3. Build the website locally and upload `website/dist/*` to `/srv/osir/site/`.
   For example, from WSL/Linux:

   ```bash
   cd website && npm run build
   scp -r dist/. root@SERVER_IP:/srv/osir/site/
   ```

## Checks

```bash
curl -I https://app.osirclaw.com/latest/manifest
curl -r 0-1048575 -o /dev/null -w '%{http_code} %{speed_download}\n' \
  https://app.osirclaw.com/latest/win-x64
curl -I https://app.osirclaw.com/manager/latest.json
```

The manifest and metadata must return `200`; a ranged artifact request must
return `206`. These checks are the release gate for `app.osirclaw.com`.
