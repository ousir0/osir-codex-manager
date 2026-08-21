#!/usr/bin/env python3
"""Upload DreamSkin mirror assets to an S3-compatible Rainyun bucket.

The script intentionally uploads only entries whose catalog rightsStatus is
redistributable. Metadata and previews are public-catalog inputs; restricted
ZIP packages remain source-direct or review-only and are never copied into the
public mirror by this command.
"""

import argparse
import concurrent.futures
import hashlib
import json
import mimetypes
import os
import pathlib
import socket
import ssl
import threading
import time
import urllib.parse
import urllib.request
import datetime
import hmac
import urllib.error


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def signing_key(secret, date, region, service):
    k_date = hmac.new(("AWS4" + secret).encode(), date.encode(), hashlib.sha256).digest()
    k_region = hmac.new(k_date, region.encode(), hashlib.sha256).digest()
    k_service = hmac.new(k_region, service.encode(), hashlib.sha256).digest()
    return hmac.new(k_service, b"aws4_request", hashlib.sha256).digest()


def signed_request(endpoint, bucket, access_key, secret_key, region, method, key="", data=b"", content_type="application/octet-stream", query="", attempts=4):
    parsed = urllib.parse.urlparse(endpoint)
    host = parsed.netloc
    path = "/" + bucket + ("" if not key else "/" + "/".join(urllib.parse.quote(part, safe="-_.~/") for part in key.split("/")))
    now = datetime.datetime.now(datetime.timezone.utc)
    amz_date = now.strftime("%Y%m%dT%H%M%SZ")
    short_date = now.strftime("%Y%m%d")
    payload_hash = sha256(data)
    headers = {
        "Host": host,
        "Content-Type": content_type,
        "Content-Length": str(len(data)),
        "x-amz-content-sha256": payload_hash,
        "x-amz-date": amz_date,
    }
    signed_headers = ";".join(sorted(k.lower() for k in headers))
    canonical_headers = "".join(k.lower() + ":" + " ".join(v.strip().split()) + "\n" for k, v in sorted(headers.items(), key=lambda item: item[0].lower()))
    canonical_request = "\n".join([method, path, query, canonical_headers, signed_headers, payload_hash])
    scope = short_date + "/" + region + "/s3/aws4_request"
    string_to_sign = "AWS4-HMAC-SHA256\n" + amz_date + "\n" + scope + "\n" + sha256(canonical_request.encode())
    signature = hmac.new(signing_key(secret_key, short_date, region, "s3"), string_to_sign.encode(), hashlib.sha256).hexdigest()
    headers["Authorization"] = "AWS4-HMAC-SHA256 Credential=" + access_key + "/" + scope + ", SignedHeaders=" + signed_headers + ", Signature=" + signature
    url = "https://" + host + path + (("?" + query) if query else "")
    request = urllib.request.Request(url, data=data if method in ("PUT", "POST") else None, method=method, headers=headers)
    retryable = (urllib.error.URLError, TimeoutError, ConnectionResetError, socket.timeout, ssl.SSLError)
    for attempt in range(1, attempts + 1):
        try:
            with urllib.request.urlopen(request, timeout=120) as response:
                return response.status, response.read(), dict(response.headers.items())
        except urllib.error.HTTPError as error:
            body = error.read()
            if error.code not in (429, 500, 502, 503, 504) or attempt == attempts:
                return error.code, body, dict(error.headers.items())
        except retryable:
            if attempt == attempts:
                raise
        time.sleep((2, 5, 10)[attempt - 1])
    raise RuntimeError("request retry loop exited unexpectedly")


def put_object(endpoint, bucket, access_key, secret_key, region, key, data, content_type):
    status, body, _ = signed_request(endpoint, bucket, access_key, secret_key, region, "PUT", key, data, content_type)
    if status not in (200, 201, 204):
        raise RuntimeError("upload failed " + str(status) + " for " + key + ": " + body.decode(errors="replace")[:300])


def object_has_size(endpoint, bucket, access_key, secret_key, region, key, expected_size):
    status, _, headers = signed_request(
        endpoint, bucket, access_key, secret_key, region, "HEAD", key
    )
    if status == 404:
        return False
    if status != 200:
        raise RuntimeError("head failed " + str(status) + " for " + key)
    try:
        return int(headers.get("Content-Length", "-1")) == expected_size
    except ValueError:
        return False


def ensure_bucket(endpoint, bucket, access_key, secret_key, region, public_read):
    status, body, _ = signed_request(endpoint, bucket, access_key, secret_key, region, "HEAD")
    if status == 404:
        status, body, _ = signed_request(endpoint, bucket, access_key, secret_key, region, "PUT", data=b"", content_type="application/octet-stream")
    if status not in (200, 201, 204, 409):
        raise RuntimeError("create/head bucket failed " + str(status) + ": " + body.decode(errors="replace")[:500])
    if public_read:
        policy = {
            "Version": "2012-10-17",
            "Statement": [{
                "Sid": "PublicReadCodexSkins",
                "Effect": "Allow",
                "Principal": "*",
                "Action": ["s3:GetObject"],
                "Resource": ["arn:aws:s3:::" + bucket + "/codex-skins/*"],
            }],
        }
        encoded = json.dumps(policy, separators=(",", ":")).encode()
        status, body, _ = signed_request(endpoint, bucket, access_key, secret_key, region, "PUT", data=encoded, content_type="application/json", query="policy=")
        if status not in (200, 204):
            raise RuntimeError("put bucket policy failed " + str(status) + ": " + body.decode(errors="replace")[:500])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("staging")
    parser.add_argument("--prefix", default="codex-skins/dreamskin/v1")
    parser.add_argument("--workers", type=int, default=4)
    parser.add_argument("--create-bucket", action="store_true")
    parser.add_argument("--public-read", action="store_true")
    parser.add_argument("--no-skip-existing", action="store_true")
    args = parser.parse_args()
    root = pathlib.Path(args.staging).resolve()
    manifest = json.loads((root / "index.json").read_text())
    endpoint = os.environ["RAINS3_ENDPOINT"].rstrip("/")
    bucket = os.environ["RAINS3_BUCKET"]
    access_key = os.environ["RAINS3_ACCESS_KEY_ID"]
    secret_key = os.environ["RAINS3_SECRET_ACCESS_KEY"]
    region = os.environ.get("RAINS3_REGION", "auto")
    if args.create_bucket:
        ensure_bucket(endpoint, bucket, access_key, secret_key, region, args.public_read)
        print("bucket ready", bucket, flush=True)
    files = [(root / "index.json", args.prefix + "/index.json", "application/json")]
    for path in sorted((root / "metadata").glob("*.json")):
        files.append((path, args.prefix + "/metadata/" + path.name, "application/json"))
    for path in sorted((root / "previews").glob("*")):
        content_type = mimetypes.guess_type(path.name)[0] or "image/jpeg"
        files.append((path, args.prefix + "/previews/" + path.name, content_type))
    allowed = {item["id"] for item in manifest["skins"] if item.get("rightsStatus") == "redistributable"}
    for item in manifest["skins"]:
        if item["id"] not in allowed:
            continue
        path = root / item["pack"]
        if path.is_file():
            files.append((path, args.prefix + "/" + item["pack"], "application/zip"))
    lock = threading.Lock()
    uploaded = 0
    skipped = 0
    failures = []
    def upload(entry):
        nonlocal uploaded, skipped
        path, key, content_type = entry
        # JSON may change without changing byte length, so always refresh it. Large
        # immutable previews/packages can be skipped safely when the size matches.
        if not args.no_skip_existing and content_type != "application/json":
            if object_has_size(endpoint, bucket, access_key, secret_key, region, key, path.stat().st_size):
                with lock:
                    skipped += 1
                return
        put_object(endpoint, bucket, access_key, secret_key, region, key, path.read_bytes(), content_type)
        with lock:
            uploaded += 1
            processed = uploaded + skipped
            if processed % 25 == 0 or processed == len(files):
                print("processed", processed, "/", len(files), "uploaded", uploaded, "skipped", skipped, flush=True)
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, args.workers)) as pool:
        futures = {pool.submit(upload, entry): entry for entry in files}
        for future in concurrent.futures.as_completed(futures):
            try:
                future.result()
            except Exception as error:
                failures.append({"key": futures[future][1], "error": str(error)})
    result = {
        "objects": len(files),
        "uploaded": uploaded,
        "skipped": skipped,
        "failed": len(failures),
        "failures": failures[:20],
        "redistributable_packages": len(allowed),
        "prefix": args.prefix,
    }
    print(json.dumps(result, ensure_ascii=False))
    if failures:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
