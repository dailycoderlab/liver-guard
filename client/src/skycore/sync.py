"""Authenticated HTTP client for the Sky synchronization service."""
from __future__ import annotations

import json
import urllib.error
import urllib.parse
import urllib.request
from typing import Any


class SyncError(Exception):
    def __init__(self, status: int, body: Any):
        self.status = status
        self.body = body
        super().__init__(f"HTTP {status}: {body}")


class SyncClient:
    def __init__(self, server: str, token: str | None = None):
        url = urllib.parse.urlsplit(server.rstrip('/'))
        if url.scheme not in ('http', 'https') or not url.hostname or url.username or url.password or url.query or url.fragment:
            raise ValueError('Invalid server URL')
        if url.scheme != 'https' and url.hostname not in ('localhost', '127.0.0.1', '::1'):
            raise ValueError('HTTPS is required except for localhost')
        self.server = server.rstrip('/')
        self.token = token

    def request(self, method: str, path: str, body: Any = None, content_type: str = 'application/json', raw: bool = False) -> Any:
        headers = {'Accept': 'application/json'}
        if self.token:
            headers['Authorization'] = f'Bearer {self.token}'
        data = None
        if body is not None:
            data = body if isinstance(body, bytes) else json.dumps(body).encode('utf-8')
            headers['Content-Type'] = content_type
        request = urllib.request.Request(self.server + path, data=data, headers=headers, method=method)
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                result = response.read()
        except urllib.error.HTTPError as error:
            payload = error.read()
            try:
                detail = json.loads(payload)
            except (ValueError, UnicodeDecodeError):
                detail = payload.decode('utf-8', errors='replace')
            raise SyncError(error.code, detail) from error
        if raw:
            return result
        return json.loads(result) if result else {}

    def login(self, username: str, password: str) -> dict:
        result = self.request('POST', '/v1/auth/login', {'username': username, 'password': password})
        self.token = result['token']
        return result

    def register(self, username: str, password: str) -> dict:
        result = self.request('POST', '/v1/auth/register', {'username': username, 'password': password})
        if result.get('token'):
            self.token = result['token']
        return result

    def asset(self, sha: str) -> bytes:
        return self.request('GET', f'/v1/assets/{sha}', raw=True)

    def put_asset(self, sha: str, data: bytes, width: int, height: int, scale: float) -> dict:
        query = urllib.parse.urlencode({'w': width, 'h': height, 'scale': scale})
        return self.request('PUT', f'/v1/assets/{sha}?{query}', data, 'image/png')
