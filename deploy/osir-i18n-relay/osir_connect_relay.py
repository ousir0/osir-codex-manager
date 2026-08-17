#!/usr/bin/env python3
"""Small, restricted HTTPS CONNECT relay for the Codex i18n bootstrap.

The relay deliberately does not terminate TLS. It only permits one exact
upstream host, so it cannot inspect or rewrite Statsig payloads. It is not a
general-purpose proxy.
"""

from __future__ import annotations

import argparse
import asyncio
import logging
from dataclasses import dataclass


ALLOWED_HOST = "ab.chatgpt.com"
MAX_HEADER_BYTES = 16 * 1024
MAX_CONNECTIONS = 32
IDLE_TIMEOUT_SECONDS = 45
MAX_LIFETIME_SECONDS = 5 * 60
MAX_TUNNEL_BYTES = 16 * 1024 * 1024


class RelayError(Exception):
    """A client request that must be rejected without exposing details."""


@dataclass
class TunnelStats:
    client_to_upstream: int = 0
    upstream_to_client: int = 0


def parse_connect_request(data: bytes) -> tuple[str, int]:
    if len(data) > MAX_HEADER_BYTES:
        raise RelayError("request headers too large")
    separator = data.find(b"\r\n\r\n")
    if separator < 0:
        raise RelayError("incomplete proxy request")
    try:
        lines = data[:separator].decode("ascii").split("\r\n")
    except UnicodeDecodeError as exc:
        raise RelayError("proxy request is not ASCII") from exc
    if not lines:
        raise RelayError("empty proxy request")
    parts = lines[0].split()
    if len(parts) != 3 or parts[0].upper() != "CONNECT" or parts[2] != "HTTP/1.1":
        raise RelayError("only HTTP/1.1 CONNECT is supported")
    authority = parts[1]
    if authority.count(":") != 1:
        raise RelayError("target must be host:port")
    host, port_text = authority.rsplit(":", 1)
    host = host.rstrip(".").lower()
    try:
        port = int(port_text, 10)
    except ValueError as exc:
        raise RelayError("invalid target port") from exc
    if host != ALLOWED_HOST or port != 443:
        raise RelayError("target is not allowed")
    return host, port


async def read_proxy_headers(reader: asyncio.StreamReader) -> bytes:
    try:
        return await asyncio.wait_for(reader.readuntil(b"\r\n\r\n"), IDLE_TIMEOUT_SECONDS)
    except (asyncio.TimeoutError, asyncio.IncompleteReadError, asyncio.LimitOverrunError) as exc:
        raise RelayError("could not read proxy headers") from exc


async def copy_tunnel(
    source: asyncio.StreamReader,
    destination: asyncio.StreamWriter,
    counter: str,
    stats: TunnelStats,
) -> None:
    try:
        while True:
            chunk = await asyncio.wait_for(source.read(64 * 1024), IDLE_TIMEOUT_SECONDS)
            if not chunk:
                return
            if counter == "client_to_upstream":
                stats.client_to_upstream += len(chunk)
                total = stats.client_to_upstream
            else:
                stats.upstream_to_client += len(chunk)
                total = stats.upstream_to_client
            if total > MAX_TUNNEL_BYTES:
                return
            destination.write(chunk)
            await destination.drain()
    except (asyncio.TimeoutError, ConnectionError, OSError):
        return


async def close_writer(writer: asyncio.StreamWriter) -> None:
    writer.close()
    try:
        await writer.wait_closed()
    except (ConnectionError, OSError):
        pass


async def handle_client(
    client_reader: asyncio.StreamReader,
    client_writer: asyncio.StreamWriter,
    semaphore: asyncio.Semaphore,
) -> None:
    peer = client_writer.get_extra_info("peername")
    upstream_writer: asyncio.StreamWriter | None = None
    try:
        async with semaphore:
            try:
                request = await read_proxy_headers(client_reader)
                host, port = parse_connect_request(request)
            except RelayError as exc:
                logging.info("reject peer=%s reason=%s", peer, exc)
                client_writer.write(b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n")
                await client_writer.drain()
                return

            try:
                upstream_reader, upstream_writer = await asyncio.wait_for(
                    # CONNECT is a byte tunnel. The client must perform the
                    # single end-to-end TLS handshake with the upstream; the
                    # relay must not add a second TLS layer here.
                    asyncio.open_connection(host, port),
                    IDLE_TIMEOUT_SECONDS,
                )
            except (asyncio.TimeoutError, ConnectionError, OSError) as exc:
                logging.warning("upstream unavailable peer=%s error=%s", peer, type(exc).__name__)
                client_writer.write(b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\n\r\n")
                await client_writer.drain()
                return

            client_writer.write(
                b"HTTP/1.1 200 Connection Established\r\n"
                b"Proxy-Agent: OSIR-Codex-I18n-Relay\r\n\r\n"
            )
            await client_writer.drain()
            stats = TunnelStats()
            await asyncio.wait_for(
                asyncio.gather(
                    copy_tunnel(client_reader, upstream_writer, "client_to_upstream", stats),
                    copy_tunnel(upstream_reader, client_writer, "upstream_to_client", stats),
                ),
                MAX_LIFETIME_SECONDS,
            )
            logging.info(
                "closed peer=%s c2u=%d u2c=%d",
                peer,
                stats.client_to_upstream,
                stats.upstream_to_client,
            )
    except asyncio.TimeoutError:
        logging.info("closed peer=%s reason=max-lifetime", peer)
    except (ConnectionError, OSError):
        logging.info("closed peer=%s reason=connection", peer)
    finally:
        if upstream_writer is not None:
            await close_writer(upstream_writer)
        await close_writer(client_writer)


async def run(bind: str, port: int, max_connections: int) -> None:
    semaphore = asyncio.Semaphore(max_connections)
    server = await asyncio.start_server(
        lambda r, w: handle_client(r, w, semaphore),
        bind,
        port,
        limit=MAX_HEADER_BYTES,
    )
    addresses = ", ".join(str(sock.getsockname()) for sock in server.sockets or [])
    logging.info("listening on %s; allowed target=%s:443", addresses, ALLOWED_HOST)
    async with server:
        await server.serve_forever()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bind", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=3128)
    parser.add_argument("--max-connections", type=int, default=MAX_CONNECTIONS)
    args = parser.parse_args()
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
    try:
        asyncio.run(run(args.bind, args.port, args.max_connections))
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
