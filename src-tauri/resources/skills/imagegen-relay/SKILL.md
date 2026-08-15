---
name: imagegen-relay
description: Generate or edit images through the user's separately configured third-party image API. Use only when the user has enabled relay image mode in Codex App Manager.
---

# Relay Image Generation

Use this skill only when the user explicitly asks for the independent relay image API or when relay image mode is enabled.

Run the bundled script with a concise prompt:

```bash
python scripts/imagegen_relay.py generate --prompt "..."
```

For edits, pass an existing image path:

```bash
python scripts/imagegen_relay.py edit --prompt "..." --input /absolute/path/input.png
```

The script reads the key from `~/.codex/imagegen-relay.json`, writes a real PNG under `~/.codex/generated_images/relay/`, and prints a Markdown preview path. Never print the key or response base64.
