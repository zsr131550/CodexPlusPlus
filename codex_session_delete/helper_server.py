from __future__ import annotations

import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Protocol

from codex_session_delete.models import DeleteResult, DeleteStatus, SessionRef


class DeleteService(Protocol):
    def delete(self, session: SessionRef) -> DeleteResult: ...
    def undo(self, token: str) -> DeleteResult: ...
    def find_archived_thread_by_title(self, title: str) -> SessionRef | None: ...


class HelperServer(ThreadingHTTPServer):
    def __init__(
        self,
        host: str,
        port: int,
        service: DeleteService,
        *,
        allow_http_mutation: bool = False,
        http_mutation_token: str | None = None,
    ):
        self.service = service
        self.allow_http_mutation = allow_http_mutation
        self.http_mutation_token = http_mutation_token
        super().__init__((host, port), _Handler)

    @property
    def port(self) -> int:
        return int(self.server_address[1])


class _Handler(BaseHTTPRequestHandler):
    server: HelperServer

    def do_OPTIONS(self) -> None:
        self._send_json({"ok": True})

    def do_GET(self) -> None:
        if self.path == "/health":
            self._send_json({"ok": True})
            return
        self._send_json({"error": "not found"}, status=404)

    def do_POST(self) -> None:
        try:
            payload = self._read_json()
            if self.path in {"/delete", "/undo", "/archived-thread"} and not self._is_mutation_authorized():
                self._send_json({"error": "forbidden"}, status=403)
                return
            if self.path == "/delete":
                session = SessionRef(session_id=str(payload.get("session_id", "")), title=str(payload.get("title", "")))
                self._send_json(self.server.service.delete(session).to_dict())
                return
            if self.path == "/undo":
                token = str(payload.get("undo_token", ""))
                self._send_json(self.server.service.undo(token).to_dict())
                return
            if self.path == "/archived-thread":
                session = self.server.service.find_archived_thread_by_title(str(payload.get("title", "")))
                self._send_json({"session_id": session.session_id, "title": session.title} if session else {"session_id": "", "title": ""})
                return
            self._send_json({"error": "not found"}, status=404)
        except Exception as exc:
            result = DeleteResult(DeleteStatus.FAILED, str(payload.get("session_id", "")) if "payload" in locals() else "", str(exc))
            self._send_json(result.to_dict(), status=400)

    def log_message(self, format: str, *args: object) -> None:
        return

    def _read_json(self) -> dict[str, object]:
        length = int(self.headers.get("Content-Length", "0"))
        raw = self.rfile.read(length).decode("utf-8") if length else "{}"
        return json.loads(raw)

    def _is_mutation_authorized(self) -> bool:
        if self.server.allow_http_mutation:
            return True
        token = self.server.http_mutation_token
        return bool(token and self.headers.get("X-Codex-Session-Delete-Token") == token)

    def _send_json(self, payload: dict[str, object], status: int = 200) -> None:
        data = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Headers", "Content-Type, X-Codex-Session-Delete-Token")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Private-Network", "true")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)
