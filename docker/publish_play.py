#!/usr/bin/env python3
"""Upload an Android artifact to Google Play and put it on a track.

The Play Developer API's edit → upload → track → commit, in the standard
library plus `openssl` for the RS256 signature a service-account grant
wants. What `fastlane supply` does for one artifact, without the Ruby, so
the signer image grows by nothing.

    balaur-publish-play ARTIFACT PACKAGE:TRACK SERVICE_ACCOUNT_JSON

ARTIFACT is an .apk or .aab; the extension picks Play's endpoint. TRACK is
internal, alpha, beta or production. PUBLISH_VERSION, if set, names the
release. The key file's *path* is the argument, never its bytes, and it is
read once, written to a 0600 file under TMPDIR for openssl, and unlinked.

Exit status is the verdict. Play's own sentence is quoted on failure, since
"the caller does not have permission" is Play's to say, not this script's.
"""

import base64
import json
import os
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request

API = "https://androidpublisher.googleapis.com/androidpublisher/v3/applications"
UPLOAD = "https://androidpublisher.googleapis.com/upload/androidpublisher/v3/applications"
SCOPE = "https://www.googleapis.com/auth/androidpublisher"
TOKEN_URI = "https://oauth2.googleapis.com/token"
TRACKS = ("internal", "alpha", "beta", "production")
TIMEOUT_S = 600


def fail(message):
    print(f"error: {message}", file=sys.stderr)
    sys.exit(1)


def log(message):
    print(message, flush=True)


def b64url(data):
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode("ascii")


def assertion(account, work):
    """The signed JWT Google trades for an access token, valid for an hour."""
    now = int(time.time())
    header = b64url(json.dumps({"alg": "RS256", "typ": "JWT"}).encode())
    claims = {
        "iss": account["client_email"],
        "scope": SCOPE,
        "aud": account.get("token_uri", TOKEN_URI),
        "iat": now,
        "exp": now + 3600,
    }
    signing_input = f"{header}.{b64url(json.dumps(claims).encode())}".encode()

    # mkstemp is 0600 by construction; the key exists on disk only for the
    # length of one openssl call, inside the container's private tmpfs.
    fd, key_path = tempfile.mkstemp(dir=work, prefix="play-", suffix=".pem")
    try:
        with os.fdopen(fd, "w") as key_file:
            key_file.write(account["private_key"])
        signed = subprocess.run(
            ["openssl", "dgst", "-sha256", "-sign", key_path],
            input=signing_input,
            capture_output=True,
        )
    finally:
        os.unlink(key_path)
    if signed.returncode != 0:
        fail("the service account's private_key is not a key openssl can sign with")
    return signing_input.decode() + "." + b64url(signed.stdout)


def google_message(raw):
    """Google's two error envelopes, reduced to the sentence worth showing."""
    try:
        detail = json.loads(raw)
    except ValueError:
        return raw.decode(errors="replace").strip() or "no detail"
    error = detail.get("error")
    if isinstance(error, dict):
        return error.get("message") or json.dumps(error)
    if isinstance(error, str):
        return ": ".join(part for part in (error, detail.get("error_description")) if part)
    return raw.decode(errors="replace").strip()


def call(method, url, body=None, headers=None, token=None):
    """One HTTP call; anything but 2xx ends the run with Play's message."""
    sent = dict(headers or {})
    if token:
        sent["Authorization"] = f"Bearer {token}"
    request = urllib.request.Request(url, data=body, method=method, headers=sent)
    try:
        with urllib.request.urlopen(request, timeout=TIMEOUT_S) as response:
            raw = response.read()
    except urllib.error.HTTPError as error:
        where = urllib.parse.urlparse(url).path
        fail(f"Play answered {error.code} to {method} {where}: {google_message(error.read())}")
    except urllib.error.URLError as error:
        fail(f"could not reach Play: {error.reason}")
    return json.loads(raw) if raw else {}


def access_token(account, work):
    body = urllib.parse.urlencode(
        {
            "grant_type": "urn:ietf:params:oauth:grant-type:jwt-bearer",
            "assertion": assertion(account, work),
        }
    ).encode()
    headers = {"Content-Type": "application/x-www-form-urlencoded"}
    answer = call("POST", account.get("token_uri", TOKEN_URI), body, headers)
    return answer["access_token"]


def upload(token, package, edit, path):
    """Streams the file; Play answers with the version code it read from it."""
    if path.endswith(".aab"):
        kind, content_type = "bundles", "application/octet-stream"
    else:
        kind, content_type = "apks", "application/vnd.android.package-archive"
    url = f"{UPLOAD}/{package}/edits/{edit}/{kind}?uploadType=media"
    headers = {"Content-Type": content_type, "Content-Length": str(os.path.getsize(path))}
    with open(path, "rb") as artifact:
        answer = call("POST", url, artifact, headers, token)
    return answer["versionCode"]


def release(token, package, edit, track, version_code, name):
    entry = {"versionCodes": [str(version_code)], "status": "completed"}
    if name:
        entry["name"] = name
    body = json.dumps({"track": track, "releases": [entry]}).encode()
    url = f"{API}/{package}/edits/{edit}/tracks/{track}"
    call("PUT", url, body, {"Content-Type": "application/json"}, token)


def load_account(path):
    try:
        with open(path) as key_file:
            account = json.load(key_file)
    except (OSError, ValueError):
        fail("play_service_account is not the JSON key Google Cloud downloads")
    for field in ("client_email", "private_key"):
        if not isinstance(account.get(field), str):
            fail(f"play_service_account has no {field}; is it a service account key?")
    return account


def main(argv):
    if len(argv) != 4:
        fail("usage: balaur-publish-play ARTIFACT PACKAGE:TRACK SERVICE_ACCOUNT_JSON")
    path, target, account_path = argv[1:]
    package, _, track = target.rpartition(":")
    if not package or track not in TRACKS:
        fail(f"{target} is not package:track with a track Play has ({', '.join(TRACKS)})")
    if not os.path.isfile(path):
        fail(f"no artifact at {path}")
    work = os.environ.get("TMPDIR", "/work")
    account = load_account(account_path)

    log(f"==> token as {account['client_email']}")
    token = access_token(account, work)
    log(f"==> edit on {package}")
    edit = call("POST", f"{API}/{package}/edits", b"", None, token)["id"]
    log(f"==> upload {os.path.basename(path)}")
    version_code = upload(token, package, edit, path)
    log(f"==> version code {version_code} -> {track}")
    release(token, package, edit, track, version_code, os.environ.get("PUBLISH_VERSION"))
    call("POST", f"{API}/{package}/edits/{edit}:commit", b"", None, token)
    log(f"==> committed {version_code} on {package}:{track}")


if __name__ == "__main__":
    main(sys.argv)
