<goal>
Deliver an independently owned OSIR Codex Manager release whose product identity, application bundle, icons, installer metadata, runtime URLs, updater trust chain, source repository, website, downloads, and deployment resources are controlled by OSIR. The primary application/download domain is https://app.osirclaw.com and the API provider endpoint is https://api.osirclaw.com/v1.
</goal>

<context>
Read SPEC.md, docs/CODEX_MANAGER_SESSIONS_HANDOFF_CN.md, docs/OSIR_API_MANAGER_BOUNDARY_CN.md, src-tauri/tauri.conf.json, src-tauri/Cargo.toml, src-tauri/src/app/paths.rs, src-tauri/src/app/codex_config.rs, src-tauri/src/state.rs, src/services/managerApi.ts, src/app/i18n.tsx, .github/workflows/release.yml, deploy/download-server/, deploy/osir-i18n-relay/, cloudflare/manager-download-router/, and the deployment tooling in /Users/ouwei/sub2api-new/tools. Use npm run audit:ownership to discover remaining legacy ownership references.
</context>

<constraints>
- Preserve unrelated user changes in both repositories.
- Do not reuse the former project's updater private key, cloud credentials, signing identities, GitHub account, domain, mirror, API, skin catalog, or relay.
- Keep legacy AWAI identifiers only in explicit, tested migration compatibility code.
- Never commit API keys, updater private keys, certificate private keys, object-storage credentials, SSH keys, or production environment files.
- Store the updater private key outside the repository and expose only its public key to the client.
- Use app.osirclaw.com as the primary website, download, updater, skin, current-package, and relay origin.
- Use api.osirclaw.com/v1 as the OSIR provider endpoint.
- Use the existing OSIR server release layout first; GitHub Releases is a backup, not the only runtime download source.
- Reuse the existing OSIR object-storage method only through isolated prefixes/buckets and repository/environment secrets.
- Do not publish a production release until DNS, TLS, signing, artifact hashes, and rollback checks pass.
</constraints>

<done_when>
1. package.json, Cargo.toml, Cargo.lock, tauri.conf.json, the installer, menus, diagnostics, logs, website, README, icons, and release workflows identify the product as OSIR Codex Manager.
2. The main binary is osir-codex-manager and the bundle identifier is com.osir.codexmanager.
3. Old manager data and the legacy awai provider migrate automatically, idempotently, and without deleting a newer OSIR destination or API key.
4. The updater public key in tauri.conf.json belongs to the locally secured OSIR private key, and GitHub Actions receives the matching private key/password through secrets only.
5. Runtime defaults use app.osirclaw.com, api.osirclaw.com/v1, and OSIR-owned repositories/resources; npm run audit:ownership:strict reports zero unapproved findings.
6. OSIR logo artwork is used for the app icon, macOS/Windows bundle icons, README assets, favicon, and NSIS header/sidebar images.
7. https://app.osirclaw.com serves the OSIR website, /manager/latest.json, versioned manager artifacts, /skins, current Codex metadata/packages, and /i18n-tunnel with valid TLS.
8. The source is committed and pushed to an ousir0-owned GitHub repository; release secrets and protected release environment are configured without exposing secret values.
9. npm run check, npm run lint, npm test, npm run test:release, cargo test --manifest-path src-tauri/Cargo.toml --lib, npm run build, and git diff --check pass.
10. A local macOS OSIR package builds and its Info.plist, bundle identifier, binary name, icons, and updater configuration are verified; Windows x64 packaging is exercised by GitHub Actions.
</done_when>

<workflow>
1. Freeze and audit both repositories and external deployment state.
2. Migrate product identity, runtime URLs, icons, installer metadata, local data, and provider configuration.
3. Generate the OSIR updater keypair and configure the public key locally.
4. Create the OSIR GitHub repository, push reviewed source, and configure non-secret variables plus protected secrets.
5. Stage app.osirclaw.com static/download/relay resources on the OSIR server without affecting the existing osirclaw.com/API services.
6. Add DNS and TLS, then publish the website and initial artifacts.
7. Run focused tests, full tests, package builds, artifact inspection, network ownership audit, and rollback checks.
8. Publish only after every done_when item is evidenced.
</workflow>

<verification_loop>
- Run npm run audit:ownership:strict after each ownership migration slice.
- Run npm run check and focused frontend/Rust tests after code edits.
- Run npm test, npm run test:release, and cargo test --manifest-path src-tauri/Cargo.toml --lib before packaging.
- Run npm run build and npm run tauri:build -- --target aarch64-apple-darwin for the local macOS artifact.
- Inspect the generated app with defaults read on Info.plist, codesign -dv, file, shasum -a 256, and the packaged smoke script where applicable.
- Verify app.osirclaw.com with DNS lookup, TLS certificate SAN, HTTP HEAD/range requests, latest.json parsing, artifact hashes, skin catalog fetch, and relay health.
- If a deployment or release check fails, keep the previous server current link and latest.json unchanged.
</verification_loop>

<execution_rules>
- Check git status before edits.
- Preserve unrelated user changes.
- Prefer rg over grep when available.
- Use the runtime patch/edit tool for manual text edits.
- Read context files before implementation.
- Batch independent reads and verification where safe.
- Run focused tests before broad tests.
- Do not paper over failures or expose secrets in output.
- Do not widen scope beyond OSIR Codex Manager ownership, release, hosting, and verification.
- Keep user updates concise and outcome-first.
</execution_rules>

<output_contract>
Deliver the reviewed source repository, GOAL.md/SPEC.md and deployment documentation, OSIR-branded application and installer assets, a verified macOS package, Windows packaging workflow evidence, app.osirclaw.com deployment evidence, GitHub repository/release configuration, hashes, known signing limitations, and one concise final handoff stating what is complete and what requires the user's credentialed action.
</output_contract>
