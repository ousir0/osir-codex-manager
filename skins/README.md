# Codex Manager Skin Catalog

This directory is the source of `https://app.osirclaw.com/skins`. It contains
editable theme sources, generated `.codexskin` packages, previews, and the
SHA-256 catalog consumed by Codex Manager.

Rebuild from an authorized source checkout:

```bash
node scripts/build-owned-skin-catalog.mjs --source /path/to/source-skins --output skins
```

Theme packages contain visual styles only. API configuration, updater settings,
client identity, and other runtime behavior remain controlled by Codex Manager.
