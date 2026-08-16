#!/usr/bin/env python3
"""Minimal OpenAI-compatible image generation client for the relay skill."""
import argparse
import base64
import binascii
import json
import os
import pathlib
import re
import sys
import urllib.error
import urllib.parse
import urllib.request
import uuid


IMAGE_MIME_TYPES = {
    ".png": "image/png",
    ".jpg": "image/jpeg",
    ".jpeg": "image/jpeg",
    ".webp": "image/webp",
    ".gif": "image/gif",
}


def config_path() -> pathlib.Path:
    return pathlib.Path.home() / ".codex" / "imagegen-relay.json"


def load_config() -> tuple[str, str, str]:
    path = config_path()
    try:
        config = json.loads(path.read_text(encoding="utf-8"))
        base_url = str(config["base_url"]).rstrip("/")
        api_key = str(config["api_key"])
        model = str(config.get("model") or "gpt-image-2")
    except (OSError, ValueError, KeyError) as exc:
        raise RuntimeError(f"独立生图 API 尚未配置：{path}") from exc
    if not base_url or not api_key:
        raise RuntimeError("独立生图 API 配置不完整")
    return base_url, api_key, model


def image_file(input_path: str) -> tuple[pathlib.Path, str, bytes]:
    path = pathlib.Path(input_path)
    mime = IMAGE_MIME_TYPES.get(path.suffix.lower())
    if mime is None:
        raise RuntimeError("参考图片仅支持 PNG、JPEG、WebP 或 GIF")
    try:
        data = path.read_bytes()
    except OSError as exc:
        raise RuntimeError(f"无法读取参考图片：{path}") from exc
    return path, mime, data


def multipart_edit_body(model: str, prompt: str, input_paths: list[str]) -> tuple[bytes, str]:
    if not input_paths:
        raise RuntimeError("edit 至少需要一张参考图片")
    boundary = f"----awai-image-edit-{uuid.uuid4().hex}"
    body = bytearray()

    def field(name: str, value: str) -> None:
        body.extend(f"--{boundary}\r\n".encode("ascii"))
        body.extend(f'Content-Disposition: form-data; name="{name}"\r\n\r\n'.encode("ascii"))
        body.extend(value.encode("utf-8"))
        body.extend(b"\r\n")

    field("model", model)
    field("prompt", prompt)
    image_field = "image" if len(input_paths) == 1 else "image[]"
    for input_path in input_paths:
        path, mime, image = image_file(input_path)
        filename = path.name.replace('"', "_")
        body.extend(f"--{boundary}\r\n".encode("ascii"))
        body.extend(
            f'Content-Disposition: form-data; name="{image_field}"; filename="{filename}"\r\n'.encode(
                "utf-8"
            )
        )
        body.extend(f"Content-Type: {mime}\r\n\r\n".encode("ascii"))
        body.extend(image)
        body.extend(b"\r\n")
    body.extend(f"--{boundary}--\r\n".encode("ascii"))
    return bytes(body), f"multipart/form-data; boundary={boundary}"


def response_image_source(result: object) -> tuple[str, str]:
    roots = [result]
    if isinstance(result, dict) and isinstance(result.get("result"), (dict, list)):
        roots.append(result["result"])
    items: list[object] = []
    for root in roots:
        if isinstance(root, list):
            items.extend(root)
        elif isinstance(root, dict):
            items.append(root)
            for key in ("data", "images", "output"):
                value = root.get(key)
                if isinstance(value, list):
                    items.extend(value)
                elif isinstance(value, (dict, str)):
                    items.append(value)
    for item in items:
        if isinstance(item, str) and item.strip():
            return "auto", item.strip()
        if not isinstance(item, dict):
            continue
        for key in ("b64_json", "base64", "image_base64", "image"):
            value = item.get(key)
            if isinstance(value, str) and value.strip():
                return "base64", value.strip()
        for key in ("url", "image_url"):
            value = item.get(key)
            if isinstance(value, dict):
                value = value.get("url")
            if isinstance(value, str) and value.strip():
                return "url", value.strip()
    raise RuntimeError("Image API response did not contain an image payload")


def url_origin(value: str) -> tuple[str, str, int | None]:
    parsed = urllib.parse.urlparse(value)
    port = parsed.port
    if port is None:
        port = 443 if parsed.scheme == "https" else 80 if parsed.scheme == "http" else None
    return parsed.scheme.lower(), (parsed.hostname or "").lower(), port


def response_image_bytes(result: object, base_url: str, api_key: str) -> bytes:
    kind, value = response_image_source(result)
    if value.startswith("data:"):
        match = re.match(r"^data:image/[^;,]+;base64,(.+)$", value, re.IGNORECASE | re.DOTALL)
        if not match:
            raise RuntimeError("Image API returned an unsupported data URI")
        value = match.group(1)
        kind = "base64"
    elif kind == "auto":
        kind = "url" if value.startswith(("https://", "http://")) else "base64"
    if kind == "url":
        parsed = urllib.parse.urlparse(value)
        if parsed.scheme not in ("http", "https"):
            raise RuntimeError("Image API returned an unsupported image URL")
        headers = {"User-Agent": "Codex-App-Manager/0.5"}
        if url_origin(value) == url_origin(base_url):
            headers["Authorization"] = f"Bearer {api_key}"
        try:
            with urllib.request.urlopen(urllib.request.Request(value, headers=headers), timeout=180) as response:
                return response.read()
        except (urllib.error.URLError, OSError) as exc:
            raise RuntimeError(f"Image URL download failed: {exc}") from exc
    try:
        return base64.b64decode(re.sub(r"\s+", "", value), validate=True)
    except (ValueError, binascii.Error) as exc:
        raise RuntimeError("Image API returned invalid base64 image data") from exc


def image_extension(data: bytes) -> str:
    if data.startswith(b"\x89PNG\r\n\x1a\n"):
        return ".png"
    if data.startswith(b"\xff\xd8\xff"):
        return ".jpg"
    if data.startswith((b"GIF87a", b"GIF89a")):
        return ".gif"
    if len(data) >= 12 and data.startswith(b"RIFF") and data[8:12] == b"WEBP":
        return ".webp"
    if len(data) >= 12 and data[4:12] in (b"ftypavif", b"ftypavis"):
        return ".avif"
    raise RuntimeError("Image API payload is not a supported image file")


def request_image(mode: str, prompt: str, input_paths: list[str] | None) -> pathlib.Path:
    base_url, api_key, model = load_config()
    endpoint = f"{base_url}/images/generations"
    if mode == "edit":
        if not input_paths:
            raise RuntimeError("edit 需要至少一个 --input 图片路径")
        request_body, content_type = multipart_edit_body(model, prompt, input_paths)
        endpoint = f"{base_url}/images/edits"
    else:
        payload: dict[str, object] = {"prompt": prompt, "model": model}
        request_body = json.dumps(payload).encode("utf-8")
        content_type = "application/json"
    request = urllib.request.Request(
        endpoint,
        data=request_body,
        headers={"Authorization": f"Bearer {api_key}", "Content-Type": content_type},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=180) as response:
            result = json.loads(response.read().decode("utf-8"))
    except (urllib.error.URLError, json.JSONDecodeError, OSError) as exc:
        raise RuntimeError(f"独立生图 API 请求失败：{exc}") from exc
    image = response_image_bytes(result, base_url, api_key)
    extension = image_extension(image)
    output_dir = pathlib.Path.home() / ".codex" / "generated_images" / "relay"
    output_dir.mkdir(parents=True, exist_ok=True)
    output = output_dir / f"image-{uuid.uuid4().hex[:12]}{extension}"
    output.write_bytes(image)
    return output


def main() -> int:
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="mode", required=True)
    for mode in ("generate", "edit"):
        command = sub.add_parser(mode)
        command.add_argument("--prompt", required=True)
        if mode == "edit":
            command.add_argument("--input", action="append", required=True)
    args = parser.parse_args()
    try:
        output = request_image(args.mode, args.prompt, getattr(args, "input", None))
    except RuntimeError as exc:
        print(str(exc), file=sys.stderr)
        return 1
    print(f"![preview]({output.as_posix()})")
    print(output.as_posix())
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
