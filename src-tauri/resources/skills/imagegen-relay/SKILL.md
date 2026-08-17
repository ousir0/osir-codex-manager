---
name: imagegen-relay
description: Generate or edit images through the user's separately configured third-party image API. Use only when the user has enabled relay image mode in OSIR Codex Manager.
---

# Relay Image Generation

Use this skill only when the user explicitly asks for the independent relay image API or when relay image mode is enabled.

On Windows, use the bundled PowerShell helper. It uses the system `curl.exe` and does not require Python:

```powershell
powershell -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File scripts/imagegen_relay.ps1 generate -Prompt "..."
```

For edits, repeat `-InputPath` in order. One image is uploaded as `image`; multiple images are uploaded as repeated `image[]` fields:

```powershell
powershell -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -Command "& 'scripts/imagegen_relay.ps1' edit -Prompt '...' -InputPath @('C:\absolute\front.png','C:\absolute\back.png')"
```

On macOS or Linux, use the Python fallback when Python 3 is available:

```bash
python scripts/imagegen_relay.py generate --prompt "..."
```

For edits, pass one or more existing image paths:

```bash
python scripts/imagegen_relay.py edit --prompt "..." --input /absolute/path/input.png

# Multiple reference images
python scripts/imagegen_relay.py edit --prompt "..." --input /absolute/path/front.png --input /absolute/path/back.png
```

The helpers read the key and selected model from `~/.codex/imagegen-relay.json` (default model: `gpt-image-2`), save the verified PNG, JPEG, WebP, GIF, or AVIF result under `~/.codex/generated_images/relay/`, and print a Markdown preview path. Compatible responses may use `b64_json`, `base64`, `image_base64`, `url`, or `image_url` inside `data`, `images`, `output`, or `result`. Never print the key or response base64.
