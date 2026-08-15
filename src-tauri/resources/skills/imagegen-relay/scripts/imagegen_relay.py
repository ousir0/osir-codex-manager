#!/usr/bin/env python3
"""Minimal OpenAI-compatible image generation client for the relay skill."""
import argparse
import base64
import json
import os
import pathlib
import sys
import urllib.error
import urllib.request


def config_path() -> pathlib.Path:
    return pathlib.Path.home() / ".codex" / "imagegen-relay.json"


def load_config() -> tuple[str, str]:
    path = config_path()
    try:
        config = json.loads(path.read_text(encoding="utf-8"))
        base_url = str(config["base_url"]).rstrip("/")
        api_key = str(config["api_key"])
    except (OSError, ValueError, KeyError) as exc:
        raise RuntimeError(f"独立生图 API 尚未配置：{path}") from exc
    if not base_url or not api_key:
        raise RuntimeError("独立生图 API 配置不完整")
    return base_url, api_key


def request_image(mode: str, prompt: str, input_path: str | None) -> pathlib.Path:
    base_url, api_key = load_config()
    payload: dict[str, object] = {"prompt": prompt, "model": "gpt-image-1"}
    endpoint = f"{base_url}/images/generations"
    if mode == "edit":
        if not input_path:
            raise RuntimeError("edit 需要 --input 图片路径")
        # Most compatible relays expose edits as JSON with base64 image data.
        payload["image"] = base64.b64encode(pathlib.Path(input_path).read_bytes()).decode("ascii")
        endpoint = f"{base_url}/images/edits"
    request = urllib.request.Request(
        endpoint,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Authorization": f"Bearer {api_key}", "Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=180) as response:
            result = json.loads(response.read().decode("utf-8"))
    except (urllib.error.URLError, json.JSONDecodeError, OSError) as exc:
        raise RuntimeError(f"独立生图 API 请求失败：{exc}") from exc
    data = result.get("data") or []
    if not data or not data[0].get("b64_json"):
        raise RuntimeError("独立生图 API 未返回 b64_json 图片数据")
    output_dir = pathlib.Path.home() / ".codex" / "generated_images" / "relay"
    output_dir.mkdir(parents=True, exist_ok=True)
    output = output_dir / "image.png"
    output.write_bytes(base64.b64decode(data[0]["b64_json"]))
    return output


def main() -> int:
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="mode", required=True)
    for mode in ("generate", "edit"):
        command = sub.add_parser(mode)
        command.add_argument("--prompt", required=True)
        if mode == "edit":
            command.add_argument("--input", required=True)
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
