#!/usr/bin/env python3
"""Private RainS3 redirect signer for the DreamSkin community mirror."""

import datetime
import hashlib
import hmac
import json
import os
import re
import urllib.parse
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


ENDPOINT = os.environ["DREAMSKIN_S3_ENDPOINT"].rstrip("/")
BUCKET = os.environ["DREAMSKIN_S3_BUCKET"]
REGION = os.environ.get("DREAMSKIN_S3_REGION", "auto")
ACCESS_KEY = os.environ["DREAMSKIN_S3_ACCESS_KEY_ID"]
SECRET_KEY = os.environ["DREAMSKIN_S3_SECRET_ACCESS_KEY"]
PREFIX = os.environ.get("DREAMSKIN_S3_PREFIX", "codex-skins/dreamskin/v1").strip("/")
TTL = min(86400, max(60, int(os.environ.get("DREAMSKIN_SIGNED_URL_TTL", "3600"))))
LISTEN = os.environ.get("DREAMSKIN_SIGNER_LISTEN", "127.0.0.1")
PORT = int(os.environ.get("DREAMSKIN_SIGNER_PORT", "3132"))
PUBLIC_PREFIX = "/skins/dreamskin/"
SAFE_REL = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._/-]{0,511}$")


def _hmac(key, value):
    return hmac.new(key, value.encode(), hashlib.sha256).digest()


def _signing_key(secret, date, region, service):
    return _hmac(_hmac(_hmac(_hmac(("AWS4" + secret).encode(), date), region), service), "aws4_request")


def presign(key, method="GET"):
    parsed = urllib.parse.urlparse(ENDPOINT)
    host = BUCKET + "." + parsed.netloc
    path = "/" + "/".join(urllib.parse.quote(part, safe="-_.~") for part in key.split("/"))
    now = datetime.datetime.now(datetime.timezone.utc)
    amz_date = now.strftime("%Y%m%dT%H%M%SZ")
    short_date = now.strftime("%Y%m%d")
    scope = short_date + "/" + REGION + "/s3/aws4_request"
    query = {
        "X-Amz-Algorithm": "AWS4-HMAC-SHA256",
        "X-Amz-Credential": ACCESS_KEY + "/" + scope,
        "X-Amz-Date": amz_date,
        "X-Amz-Expires": str(TTL),
        "X-Amz-SignedHeaders": "host",
    }
    canonical_query = urllib.parse.urlencode(sorted(query.items()), quote_via=urllib.parse.quote, safe="~")
    canonical_request = method + "\n" + path + "\n" + canonical_query + "\nhost:" + host + "\n\nhost\nUNSIGNED-PAYLOAD"
    string_to_sign = "AWS4-HMAC-SHA256\n" + amz_date + "\n" + scope + "\n" + hashlib.sha256(canonical_request.encode()).hexdigest()
    signature = hmac.new(_signing_key(SECRET_KEY, short_date, REGION, "s3"), string_to_sign.encode(), hashlib.sha256).hexdigest()
    return "https://" + host + path + "?" + canonical_query + "&X-Amz-Signature=" + signature


def relative_path(raw_path):
    path = urllib.parse.urlparse(raw_path).path
    if not path.startswith(PUBLIC_PREFIX):
        return None
    rel = urllib.parse.unquote(path[len(PUBLIC_PREFIX):]).strip("/")
    if not rel or ".." in rel or not SAFE_REL.fullmatch(rel):
        return None
    return rel


class Handler(BaseHTTPRequestHandler):
    server_version = "DreamSkinSigner/1"

    def do_HEAD(self):
        self._handle(head=True)

    def do_GET(self):
        self._handle(head=False)

    def _handle(self, head):
        if self.path == "/health":
            body = b'{"service":"dreamskin-signer","status":"ok"}'
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            if not head:
                self.wfile.write(body)
            return
        rel = relative_path(self.path)
        if rel is None:
            self.send_error(404)
            return
        key = PREFIX + "/" + rel
        if rel == "index.json":
            try:
                with urllib.request.urlopen(presign(key), timeout=15) as response:
                    body = response.read(2 * 1024 * 1024 + 1)
                if len(body) > 2 * 1024 * 1024:
                    raise RuntimeError("catalog too large")
                json.loads(body)
            except Exception as error:
                self.send_error(502, str(error))
                return
            self.send_response(200)
            self.send_header("Content-Type", "application/json; charset=utf-8")
            self.send_header("Cache-Control", "public, max-age=300")
            self.send_header("Access-Control-Allow-Origin", "*")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            if not head:
                self.wfile.write(body)
            return
        self.send_response(302)
        self.send_header("Location", presign(key, "HEAD" if head else "GET"))
        self.send_header("Cache-Control", "private, max-age=300")
        self.end_headers()

    def log_message(self, fmt, *args):
        print(fmt % args, flush=True)


if __name__ == "__main__":
    ThreadingHTTPServer((LISTEN, PORT), Handler).serve_forever()
