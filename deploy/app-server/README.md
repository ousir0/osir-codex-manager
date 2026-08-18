# app.osirclaw.com deployment

This directory defines the isolated OSIR Codex Manager web/download origin. It does not modify the existing `osirclaw.com` or `api.osirclaw.com` services.

## Layout

```text
/var/www/osir-codex-manager/
  current -> releases/<release-id>
  releases/<release-id>/
    site/
    manager/latest.json
    manager/<version>/
    manager/latest/
    latest/
    skins/
```

## Deployment order

1. Publish a candidate release with `publish-rainyun.sh`.
2. Install `nginx-http.conf` and verify it with a direct Host-header request.
3. Run `configure-alidns-app-record.sh` with the local Alibaba Cloud CLI to create/update the `A` record `app.osirclaw.com`.
4. Issue the Let's Encrypt certificate for `app.osirclaw.com`.
5. Install `nginx-https.conf`, reload Nginx, and run HTTPS/range checks.
6. Start the restricted OSIR i18n relay on `127.0.0.1:3130`.

The DNS script is idempotent and defaults to the current OSIR server IP. Override
`OSIR_DNS_VALUE` or `ALIYUN_PROFILE` when moving the service.

## Object storage

The first release uses the OSIR server plus GitHub Releases. The existing OSIR Tencent COS account can later store immutable manager artifacts under an isolated `osir-codex-manager/` prefix. Keep COS credentials only in server/GitHub secrets; do not copy them into this repository.
