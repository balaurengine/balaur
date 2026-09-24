#!/usr/bin/env python3
"""Upload an iOS build to App Store Connect and read Apple's first verdict.

The App Store Connect build upload API, which since 2025 does what
Transporter did: reserve a buildUpload, reserve a buildUploadFile, PUT the
bytes where Apple says in the parts Apple says, mark the file uploaded, and
read the upload's state. Standard library plus `openssl` for the ES256
token, so the image needs no Java and no Mac.

    balaur-publish-appstore ARTIFACT CREDS_DIR

ARTIFACT is a signed .ipa; its Info.plist supplies the bundle id and both
version strings, so nothing is typed twice. CREDS_DIR holds apple_issuer_id,
apple_key_id and apple_private_key (the .p8 as downloaded) — the three
notarisation uses. The key is read by openssl in place and never copied.

Apple processes for minutes to hours. PUBLISH_WAIT_MINUTES (default 5)
bounds the wait: a build still processing when it closes is reported as
accepted, not failed, since Apple mails the outcome. Exit status is the
verdict and Apple's own sentence is quoted on failure.
"""

import json
import os
import plistlib
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import zipfile
from base64 import urlsafe_b64encode

API = "https://api.appstoreconnect.apple.com/v1"
AUDIENCE = "appstoreconnect-v1"
TOKEN_LIFE_S = 1200
TOKEN_REUSE_S = 900
POLL_S = 30
TIMEOUT_S = 600


def fail(message):
    print(f"error: {message}", file=sys.stderr)
    sys.exit(1)


def log(message):
    print(message, flush=True)


def b64url(data):
    return urlsafe_b64encode(data).rstrip(b"=").decode("ascii")


def der_to_raw(der):
    """openssl gives an ASN.1 SEQUENCE of (r, s); JWS wants r||s, 32 bytes each."""
    if not der or der[0] != 0x30:
        fail("openssl produced no ECDSA signature; is apple_private_key the .p8 as downloaded?")
    at = 2 + (der[1] & 0x7F if der[1] & 0x80 else 0)
    halves = []
    for _ in range(2):
        if der[at] != 0x02:
            fail("openssl produced a signature this script cannot read")
        length, start = der[at + 1], at + 2
        halves.append(der[start : start + length].lstrip(b"\x00").rjust(32, b"\x00"))
        at = start + length
    return b"".join(halves)


class Session:
    """A bearer token minted from the key, reused for fifteen of its twenty minutes."""

    def __init__(self, issuer, key_id, key_path):
        self.issuer, self.key_id, self.key_path = issuer, key_id, key_path
        self.jwt, self.minted = None, 0.0

    def bearer(self):
        if time.time() - self.minted > TOKEN_REUSE_S:
            now = int(time.time())
            header = {"alg": "ES256", "kid": self.key_id, "typ": "JWT"}
            claims = {"iss": self.issuer, "iat": now, "exp": now + TOKEN_LIFE_S, "aud": AUDIENCE}
            signing_input = ".".join(b64url(json.dumps(part).encode()) for part in (header, claims))
            signed = subprocess.run(
                ["openssl", "dgst", "-sha256", "-sign", self.key_path],
                input=signing_input.encode(),
                capture_output=True,
            )
            if signed.returncode != 0:
                fail("apple_private_key is not a key openssl can sign with; it should be the .p8 as downloaded")
            self.jwt = f"{signing_input}.{b64url(der_to_raw(signed.stdout))}"
            self.minted = time.time()
        return self.jwt

    def call(self, method, path, body=None):
        headers = {"Authorization": f"Bearer {self.bearer()}", "Accept": "application/json"}
        data = None
        if body is not None:
            data = json.dumps(body).encode()
            headers["Content-Type"] = "application/json"
        return http(method, API + path, data, headers, "Apple")


def message_from(raw):
    """Apple's error envelope, reduced to the sentences worth showing."""
    try:
        detail = json.loads(raw)
    except ValueError:
        return raw.decode(errors="replace").strip() or "no detail"
    errors = detail.get("errors") if isinstance(detail, dict) else None
    if errors:
        return "; ".join(
            ": ".join(part for part in (e.get("code"), e.get("title"), e.get("detail")) if part)
            for e in errors
        )
    return raw.decode(errors="replace").strip() or "no detail"


def http(method, url, data, headers, who):
    """One HTTP call; anything but 2xx ends the run with the server's message."""
    request = urllib.request.Request(url, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(request, timeout=TIMEOUT_S) as response:
            raw = response.read()
    except urllib.error.HTTPError as error:
        where = urllib.parse.urlparse(url).path
        fail(f"{who} answered {error.code} to {method} {where}: {message_from(error.read())}")
    except urllib.error.URLError as error:
        fail(f"could not reach {who}: {error.reason}")
    return json.loads(raw) if raw else {}


def describe(path):
    """Bundle id, marketing version and build number, from the app's own Info.plist."""
    try:
        with zipfile.ZipFile(path) as ipa:
            plists = [
                name
                for name in ipa.namelist()
                if name.startswith("Payload/") and name.endswith(".app/Info.plist") and name.count("/") == 2
            ]
            if len(plists) != 1:
                fail("the .ipa has no single Payload/<App>.app/Info.plist")
            info = plistlib.loads(ipa.read(plists[0]))
    except (zipfile.BadZipFile, plistlib.InvalidFileException):
        fail("the artifact is not an .ipa this script can read")
    try:
        return info["CFBundleIdentifier"], info["CFBundleShortVersionString"], info["CFBundleVersion"]
    except KeyError as missing:
        fail(f"Info.plist has no {missing.args[0]}")


def read_credential(creds, name):
    path = os.path.join(creds, name)
    if not os.path.isfile(path):
        fail(f"missing credential: {name}")
    with open(path) as file:
        return file.read().strip()


def upload_parts(path, operations):
    """Each operation is one PUT of one byte range, with the headers Apple dictates."""
    with open(path, "rb") as artifact:
        for index, operation in enumerate(operations, 1):
            artifact.seek(operation["offset"])
            part = artifact.read(operation["length"])
            headers = {h["name"]: h["value"] for h in operation.get("requestHeaders") or []}
            headers.setdefault("Content-Type", "application/octet-stream")
            log(f"    part {index}/{len(operations)}: {len(part)} bytes")
            http(operation.get("method") or "PUT", operation["url"], part, headers, "the upload host")


def wait(session, upload_id, minutes):
    """Apple's state until COMPLETE or FAILED, or the name of the state it was left in."""
    deadline = time.time() + minutes * 60
    while True:
        attributes = session.call("GET", f"/buildUploads/{upload_id}")["data"]["attributes"]
        state = attributes.get("state") or {}
        name = state.get("state") or "PROCESSING"
        if name == "COMPLETE":
            return name
        if name == "FAILED":
            details = state.get("errors") or []
            said = "; ".join(d.get("message") or d.get("detail") or json.dumps(d) for d in details)
            fail(f"App Store Connect rejected the build: {said or 'no detail'}")
        if time.time() > deadline:
            return name
        time.sleep(POLL_S)


def main(argv):
    if len(argv) != 3:
        fail("usage: balaur-publish-appstore ARTIFACT CREDS_DIR")
    path, creds = argv[1:]
    if not path.endswith(".ipa") or not os.path.isfile(path):
        fail(f"{os.path.basename(path)} is not an .ipa; App Store Connect takes nothing else from here")
    issuer = read_credential(creds, "apple_issuer_id")
    key_id = read_credential(creds, "apple_key_id")
    key_path = os.path.join(creds, "apple_private_key")
    if not os.path.isfile(key_path):
        fail("missing credential: apple_private_key")

    bundle_id, version, build = describe(path)
    session = Session(issuer, key_id, key_path)
    log(f"==> {bundle_id} {version} ({build}) with key {key_id}")

    query = urllib.parse.urlencode({"filter[bundleId]": bundle_id, "fields[apps]": "bundleId"})
    apps = session.call("GET", f"/apps?{query}").get("data") or []
    if not apps:
        fail(f"no app with bundle id {bundle_id} is visible to this key; create it in App Store Connect first")
    app_id = apps[0]["id"]

    log("==> reserving the upload")
    upload = session.call(
        "POST",
        "/buildUploads",
        {
            "data": {
                "type": "buildUploads",
                "attributes": {
                    "cfBundleShortVersionString": version,
                    "cfBundleVersion": build,
                    "platform": "IOS",
                },
                "relationships": {"app": {"data": {"type": "apps", "id": app_id}}},
            }
        },
    )["data"]["id"]
    size = os.path.getsize(path)
    reserved = session.call(
        "POST",
        "/buildUploadFiles",
        {
            "data": {
                "type": "buildUploadFiles",
                "attributes": {
                    "fileName": os.path.basename(path),
                    "fileSize": size,
                    "assetType": "ASSET",
                    "uti": "com.apple.ipa",
                },
                "relationships": {"buildUpload": {"data": {"type": "buildUploads", "id": upload}}},
            }
        },
    )["data"]
    operations = reserved["attributes"].get("uploadOperations") or []
    if not operations:
        fail("Apple reserved the file but gave no upload operations")

    log(f"==> uploading {size} bytes in {len(operations)} part(s)")
    upload_parts(path, operations)

    log("==> marking uploaded")
    session.call(
        "PATCH",
        f"/buildUploadFiles/{reserved['id']}",
        {"data": {"type": "buildUploadFiles", "id": reserved["id"], "attributes": {"uploaded": True}}},
    )

    minutes = int(os.environ.get("PUBLISH_WAIT_MINUTES") or 5)
    log(f"==> waiting up to {minutes} min for App Store Connect")
    outcome = wait(session, upload, minutes)
    if outcome == "COMPLETE":
        log(f"==> processed: {version} ({build}) is in TestFlight")
    else:
        log(f"==> accepted: {version} ({build}) is still {outcome.lower()}; Apple mails the outcome")


if __name__ == "__main__":
    main(sys.argv)
