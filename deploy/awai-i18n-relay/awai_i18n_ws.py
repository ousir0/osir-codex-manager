#!/usr/bin/env python3
"""Restricted WebSocket-to-TCP tunnel for Codex UI localization."""

from __future__ import annotations

import asyncio
import logging

import websockets


UPSTREAM = ("ab.chatgpt.com", 443)
MAX_BYTES = 16 * 1024 * 1024
MAX_LIFETIME = 5 * 60
CONNECTIONS = asyncio.Semaphore(32)


async def bridge(websocket: websockets.WebSocketServerProtocol, path: str) -> None:
    if path != "/i18n-tunnel":
        await websocket.close(code=1008, reason="path not allowed")
        return
    async with CONNECTIONS:
        await run_tunnel(websocket)


async def run_tunnel(websocket: websockets.WebSocketServerProtocol) -> None:
    reader, writer = await asyncio.wait_for(asyncio.open_connection(*UPSTREAM), 45)
    uploaded = 0
    downloaded = 0
    await websocket.send("ready")

    async def upload() -> None:
        nonlocal uploaded
        async for message in websocket:
            if not isinstance(message, bytes):
                continue
            uploaded += len(message)
            if uploaded > MAX_BYTES:
                return
            writer.write(message)
            await writer.drain()

    async def download() -> None:
        nonlocal downloaded
        while chunk := await reader.read(64 * 1024):
            downloaded += len(chunk)
            if downloaded > MAX_BYTES:
                return
            await websocket.send(chunk)

    try:
        await asyncio.wait_for(asyncio.gather(upload(), download()), MAX_LIFETIME)
    except (asyncio.TimeoutError, ConnectionError, OSError, websockets.ConnectionClosed):
        pass
    finally:
        writer.close()
        try:
            await writer.wait_closed()
        except (ConnectionError, OSError):
            pass
        logging.info("i18n tunnel closed uploaded=%d downloaded=%d", uploaded, downloaded)


async def main() -> None:
    async with websockets.serve(
        bridge,
        "127.0.0.1",
        3130,
        max_size=MAX_BYTES,
        max_queue=8,
        ping_interval=None,
    ):
        logging.info("AWAI i18n WebSocket tunnel listening on 127.0.0.1:3130")
        await asyncio.Future()


if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
    asyncio.run(main())
