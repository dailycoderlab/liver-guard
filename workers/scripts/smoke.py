#!/usr/bin/env python3
"""Exercise the local REST API, including cross-user isolation.

Run against an isolated dev database: SKY_BASE=http://127.0.0.1:8787 python3 scripts/smoke.py
Creates two accounts and test data; use a disposable database.
"""
import hashlib
import json
import os
import time
import urllib.error
import urllib.request
import uuid
from concurrent.futures import ThreadPoolExecutor

BASE = os.environ.get("SKY_BASE", "http://127.0.0.1:8787")


def request(method, path, body=None, token=None, raw=None):
    headers = {}
    if body is not None:
        raw = json.dumps(body).encode()
        headers["content-type"] = "application/json"
    if token:
        headers["authorization"] = "Bearer " + token
    req = urllib.request.Request(BASE + path, data=raw, headers=headers, method=method)
    try:
        response = urllib.request.urlopen(req, timeout=30)
    except urllib.error.HTTPError as e:
        response = e
    with response:
        data = response.read()
        if response.headers.get("content-type", "").startswith("application/json"):
            data = json.loads(data)
        return response.status, data


def check(name, condition, result=None):
    print(("PASS " if condition else "FAIL ") + name + (f" {result}" if not condition else ""), flush=True)
    if not condition:
        raise AssertionError(name)


def main():
    suffix = uuid.uuid4().hex
    password = "smoke-password-123"
    users = []
    for i in range(2):
        username = f"smoke_{i}_{suffix}"
        status, result = request("POST", "/v1/auth/register", {"username": username, "password": password})
        check("register", status == 201 and bool(result.get("token")), (status, result))
        users.append((username, result["token"]))
    a, b = users[0][1], users[1][1]
    status, result = request("GET", "/v1/auth/me")
    check("me requires authentication", status == 401, (status, result))
    status, result = request("GET", "/v1/auth/me", token="invalid-token")
    check("me rejects invalid token", status == 401, (status, result))
    status, result = request("GET", "/v1/auth/me", token=a)
    check("legacy username me", status == 200 and result["user"]["username"] == users[0][0] and bool(result["user"]["id"]) and result["user"]["email"] is None, (status, result))
    email = f"smoke_{suffix}@example.com"
    status, result = request("POST", "/v1/auth/register", {"email": email, "password": password})
    check("register email", status == 201 and result["user"]["email"] == email and bool(result["user"]["id"]), (status, result))
    email_id, email_token = result["user"]["id"], result["token"]
    status, result = request("GET", "/v1/auth/me", token=email_token)
    check("email me", status == 200 and result["user"]["id"] == email_id and result["user"]["email"] == email, (status, result))
    status, result = request("POST", "/v1/auth/login", {"email": email.upper(), "password": password})
    check("login email case-insensitive", status == 200 and result["user"]["id"] == email_id, (status, result))
    status, result = request("POST", "/v1/auth/login", {"username": email, "password": password})
    check("login email as username", status == 200 and result["user"]["id"] == email_id, (status, result))
    status, result = request("POST", "/v1/auth/login", {"email": email, "username": email.upper(), "password": password})
    check("matching identifier fields", status == 200 and result["user"]["id"] == email_id, (status, result))
    status, result = request("POST", "/v1/auth/login", {"email": email, "username": users[0][0], "password": password})
    check("conflicting identifier fields rejected", status == 400, (status, result))
    status, result = request("POST", "/v1/auth/register", {"username": email.upper(), "password": password})
    check("duplicate email across aliases", status == 409, (status, result))
    second_email = f"smoke_alt_{suffix}@example.com"
    status, result = request("POST", "/v1/auth/register", {"username": second_email, "password": password})
    check("register email as username", status == 201 and result["user"]["email"] == second_email, (status, result))
    second_id = result["user"]["id"]
    status, result = request("POST", "/v1/auth/login", {"email": second_email, "password": password})
    check("login username-registered email", status == 200 and result["user"]["id"] == second_id, (status, result))
    status, result = request("POST", "/v1/auth/register", {"email": "not-an-email", "password": password})
    check("invalid email rejected", status == 400, (status, result))
    status, result = request("POST", "/v1/auth/register", {"username": users[0][0].upper(), "password": password})
    check("duplicate username", status == 409, (status, result))
    status, result = request("POST", "/v1/auth/login", {"username": users[0][0], "password": password})
    check("login", status == 200 and bool(result.get("token")), (status, result))
    status, result = request("POST", "/v1/auth/login", {"username": users[0][0], "password": "wrong-password"})
    check("wrong password", status == 401, (status, result))
    status, result = request("GET", "/v1/scripts")
    check("authentication required", status == 401, (status, result))
    image = b"smoke-asset-" + suffix.encode()
    sha = hashlib.sha256(image).hexdigest()
    path = f"/v1/assets/{sha}"
    status, result = request("PUT", path + "?w=10&h=10", token=a, raw=image)
    check("asset upload", status == 200, (status, result))
    for token, expected in [(None, 401), (b, 404), (a, 200)]:
        status, result = request("GET", path, token=token)
        check("asset read isolation", status == expected and (expected != 200 or result == image), (status, result))
    status, result = request("PUT", path + "?w=10&h=10", token=b, raw=image)
    check("same hash independently owned", status == 200, (status, result))
    status, result = request("POST", "/v1/scripts", {"name": "smoke"}, a)
    check("create script", status == 201, (status, result))
    sid = result["script"]["id"]
    script = f"/v1/scripts/{sid}"
    for uri in [script, script + "/versions", script + "/versions/1", script + "/bundle"]:
        status, result = request("GET", uri, token=b)
        check("script child isolation " + uri, status == 404, (status, result))
    status, result = request("PATCH", script + "/draft", {"revision": 0, "draft": {"text": "first"}}, a)
    check("save draft", status == 200 and result["revision"] == 1, (status, result))
    status, result = request("PATCH", script + "/draft", {"revision": 0, "draft": {}}, a)
    check("stale draft conflict", status == 409 and result["revision"] == 1, (status, result))
    status, result = request("PATCH", script + "/draft", {"revision": 1, "draft": {}}, b)
    check("draft isolation", status == 404, (status, result))
    ir = {"irVersion": 1, "meta": {"name": "smoke"}, "entry": "main", "blocks": {"main": []}, "targets": {}}
    status, result = request("POST", script + "/publish", {"ir": ir}, a)
    check("publish", status == 200 and result["version"] == 1, (status, result))
    with ThreadPoolExecutor(max_workers=2) as pool:
        results = list(pool.map(lambda _: request("POST", script + "/publish", {"ir": ir}, a), range(2)))
    check("concurrent publication", sorted(r[1].get("version") for r in results) == [2, 3], results)
    status, result = request("GET", script + "/versions/1", token=b)
    check("published version isolation", status == 404, (status, result))
    status, result = request("GET", script + "/bundle", token=a)
    check("bundle", status == 200 and result["version"] == 3, (status, result))
    t = int(time.time() * 1000)
    record = {"scriptId": sid, "version": 1, "state": "completed", "startedAt": t, "endedAt": t}
    status, result = request("POST", "/v1/runs", record, b)
    check("run cannot reference other user's script", status == 404, (status, result))
    status, result = request("POST", "/v1/runs", record, a)
    check("create local run record", status == 201, (status, result))
    status_delete, result_delete = request("DELETE", script, token=a)
    check("script with run cannot be deleted", status_delete == 409, (status_delete, result_delete))
    run = "/v1/runs/" + result["runId"]
    for uri in [run, run + "/events", run + "/logs", run + "/shots/a.jpg"]:
        for method in (["GET", "PUT"] if uri.endswith(("/logs", "/a.jpg")) else ["GET"]):
            status, result = request(method, uri, token=b, raw=b"x" if method == "PUT" else None)
            check("run child isolation " + method + " " + uri, status == 404, (status, result))
    status, result = request("PUT", run + "/logs", token=a, raw=b'{"event":"done"}\n')
    check("upload logs", status == 200, (status, result))
    status, result = request("GET", run + "/logs", token=a)
    check("fetch logs", status == 200 and b"done" in result, (status, result))
    status, result = request("PUT", run + "/shots/a.jpg", token=a, raw=b"image")
    check("upload screenshot", status == 200, (status, result))
    status, result = request("GET", run + "/shots/a.jpg", token=a)
    check("fetch screenshot", status == 200 and result == b"image", (status, result))
    for uri in ["/v1/ws/device", "/v1/ws/console", "/v1/devices"]:
        status, result = request("GET", uri, token=a)
        check("removed control route " + uri, status == 404, (status, result))
    print("All smoke checks passed.")


if __name__ == "__main__":
    main()
