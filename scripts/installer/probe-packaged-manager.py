#!/usr/bin/env python3
"""Probe packaged manager launch modes without exposing fixture contents."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
from typing import NoReturn
from urllib.parse import urlencode


MARKER = ".codex-plus-package-fixture"
PROFILE_MARKER = ".codex-plus-package-disposable-profile"
MODES = ("ordinary", "show-update", "provider-import")
NATIVE_EXIT_AFTER_MS = 6000
MAX_STATE_EVIDENCE_BYTES = 64 * 1024


def fail(message: str) -> NoReturn:
    raise SystemExit(f"error: {message}")


def available_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind(("127.0.0.1", 0))
        return int(listener.getsockname()[1])


def process_options() -> dict[str, object]:
    options: dict[str, object] = {
        "stdin": subprocess.DEVNULL,
        "stdout": subprocess.DEVNULL,
        "stderr": subprocess.DEVNULL,
    }
    if os.name == "nt":
        startup = subprocess.STARTUPINFO()
        startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
        startup.wShowWindow = subprocess.SW_HIDE
        options["startupinfo"] = startup
        options["creationflags"] = subprocess.CREATE_NEW_PROCESS_GROUP
    else:
        options["start_new_session"] = True
    return options


def stop_process(process: subprocess.Popen[bytes]) -> None:
    if process.poll() is not None:
        return
    if os.name == "nt":
        subprocess.run(
            ["taskkill", "/PID", str(process.pid), "/T", "/F"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=10,
            check=False,
        )
    else:
        process.terminate()
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=10)


def wait_for_file(path: Path, process: subprocess.Popen[bytes], timeout: float) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if path.is_file():
            return
        code = process.poll()
        if code is not None:
            fail(f"manager exited before becoming ready with exit code {code}")
        time.sleep(0.05)
    fail("manager did not become ready before the probe timeout")


def wait_for_exit(process: subprocess.Popen[bytes], timeout: float) -> int:
    try:
        return int(process.wait(timeout=timeout))
    except subprocess.TimeoutExpired:
        fail("manager did not exit before the probe timeout")


def provider_url() -> str:
    query = urlencode(
        {
            "resource": "provider",
            "name": "Package Fixture Provider",
            "baseUrl": "https://package-fixture.invalid/v1",
            "apiKey": "package-fixture-token",
            "wireApi": "responses",
            "relayMode": "pureApi",
        }
    )
    return f"codexplusplus://v1/import/provider?{query}"


def previous_state_directories(
    root: Path, profile_state: Path | None, binary_parent: Path
) -> list[Path]:
    candidates = [
        root / ".codex-session-delete",
        root / "user" / ".codex-session-delete",
        binary_parent / ".codex-session-delete",
    ]
    if profile_state is not None:
        candidates.insert(0, profile_state)
    return list(dict.fromkeys(candidates))


def file_signature(path: Path) -> tuple[int, int, str] | None:
    try:
        if not path.is_file():
            return None
        with path.open("rb") as stream:
            content = stream.read(MAX_STATE_EVIDENCE_BYTES + 1)
        if len(content) > MAX_STATE_EVIDENCE_BYTES:
            return None
        stat = path.stat()
    except OSError:
        return None
    return stat.st_size, stat.st_mtime_ns, hashlib.sha256(content).hexdigest()


def file_size(path: Path) -> int:
    try:
        return path.stat().st_size if path.is_file() else 0
    except OSError:
        return 0


def provider_import_evidence(
    state_directories: list[Path],
    pending_before: dict[Path, tuple[int, int, str] | None],
    diagnostic_offsets: dict[Path, int],
    process_id: int,
) -> tuple[bool, str]:
    for state in state_directories:
        pending = state / "pending-provider-import.json"
        current = file_signature(pending)
        if current is None or current == pending_before[pending]:
            continue
        try:
            request = json.loads(pending.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            continue
        if (
            request.get("name") == "Package Fixture Provider"
            and request.get("baseUrl") == "https://package-fixture.invalid/v1"
        ):
            return True, "pending-request"

    observed_events: set[str] = set()
    for state in state_directories:
        diagnostic = state / "codex-plus.log"
        offset = diagnostic_offsets[diagnostic]
        try:
            size = diagnostic.stat().st_size
            if size < offset or size - offset > MAX_STATE_EVIDENCE_BYTES:
                continue
            with diagnostic.open("rb") as stream:
                stream.seek(offset)
                records = stream.read(MAX_STATE_EVIDENCE_BYTES)
        except OSError:
            continue
        for line in records.decode("utf-8", errors="replace").splitlines():
            try:
                record = json.loads(line)
            except json.JSONDecodeError:
                continue
            if record.get("pid") != process_id:
                continue
            event = record.get("event")
            if isinstance(event, str):
                observed_events.add(event)
            if event != "manager.provider_import_url.pending":
                continue
            detail = record.get("detail")
            if isinstance(detail, dict) and (
                detail.get("name") == "Package Fixture Provider"
                and detail.get("baseUrl") == "https://package-fixture.invalid/v1"
            ):
                return True, "diagnostic-event"

    if "manager.provider_import_url.failed" in observed_events:
        return False, "manager reported a rejected provider import"
    if observed_events:
        return False, "manager emitted no provider-import acceptance event"
    return False, "manager emitted no bounded provider-import evidence"


def bounded_native_failure(root: Path, process_id: int) -> str:
    diagnostic = root / "diagnostic.jsonl"
    try:
        size = diagnostic.stat().st_size
        offset = max(0, size - MAX_STATE_EVIDENCE_BYTES)
        with diagnostic.open("rb") as stream:
            stream.seek(offset)
            records = stream.read(MAX_STATE_EVIDENCE_BYTES)
    except OSError:
        return "no bounded Native diagnostic"
    lines = records.decode("utf-8", errors="replace").splitlines()
    if offset:
        lines = lines[1:]
    for line in reversed(lines):
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            continue
        if record.get("pid") != process_id:
            continue
        event = record.get("event")
        if event != "native_manager.run_failed":
            continue
        detail = record.get("detail")
        kind = detail.get("kind") if isinstance(detail, dict) else None
        if isinstance(kind, str):
            kind = " ".join(kind.split())
            kind = kind.replace(str(root), "<fixture>")
            profile = os.environ.get("CODEX_PLUS_PACKAGE_WINDOWS_PROFILE", "")
            if profile:
                kind = kind.replace(profile, "<profile>")
            return f"{event}: {kind[:256]}"
        return event
    return "no matching Native diagnostic"


def isolated_environment(root: Path, port: int) -> dict[str, str]:
    user = root / "user"
    local = user / "local"
    roaming = user / "roaming"
    temp = root / "temp"
    webview2 = root / "webview2"
    for directory in (user, local, roaming, temp, webview2):
        directory.mkdir(parents=True, exist_ok=True)
    environment = os.environ.copy()
    environment.update(
        {
            "HOME": str(user),
            "USERPROFILE": str(user),
            "LOCALAPPDATA": str(local),
            "APPDATA": str(roaming),
            "XDG_CACHE_HOME": str(user / ".cache"),
            "XDG_CONFIG_HOME": str(user / ".config"),
            "XDG_DATA_HOME": str(user / ".local" / "share"),
            "XDG_STATE_HOME": str(user / ".local" / "state"),
            "TMP": str(temp),
            "TEMP": str(temp),
            "TMPDIR": str(temp),
            "WEBVIEW2_USER_DATA_FOLDER": str(webview2),
            "CODEX_HOME": str(root / "codex-home"),
            "CODEX_PLUS_GUARD_PORT": str(port),
        }
    )
    return environment


def native_environment(root: Path, port: int, mode: str) -> tuple[dict[str, str], Path, Path]:
    environment = isolated_environment(root, port)
    state = root / "state"
    pending = root / "pending-provider-import.json"
    report = root / f"perf-{mode}.json"
    update_metadata = root / "update-metadata.json"
    update_asset = root / "update-asset.bin"
    update_metadata.write_text(
        json.dumps({"version": "0.0.0", "body": "package fixture current release"}),
        encoding="utf-8",
    )
    update_asset.write_bytes(b"package-fixture-update")
    environment.update(
        {
            "CODEX_PLUS_NATIVE_STATE_DIR": str(state),
            "CODEX_PLUS_NATIVE_SETTINGS_PATH": str(root / "settings.json"),
            "CODEX_PLUS_NATIVE_CODEX_HOME": str(root / "codex-home"),
            "CODEX_PLUS_NATIVE_CCS_DB_PATH": str(root / "cc-switch.db"),
            "CODEX_PLUS_NATIVE_PENDING_IMPORT_PATH": str(pending),
            "CODEX_PLUS_NATIVE_BACKUP_DIR": str(root / "backups"),
            "CODEX_PLUS_NATIVE_CONTEXT_OWNERSHIP_PATH": str(
                root / "context-live-ownership.json"
            ),
            "CODEX_PLUS_NATIVE_DIAGNOSTIC_LOG_PATH": str(root / "diagnostic.jsonl"),
            "CODEX_PLUS_NATIVE_LATEST_STATUS_PATH": str(root / "latest-status.json"),
            "CODEX_PLUS_NATIVE_DESKTOP_INTEGRATION_FIXTURE_STATE": (
                "windows_needs_repair_legacy"
            ),
            "CODEX_PLUS_NATIVE_DESKTOP_INTEGRATION_RECORD_PATH": str(
                root / "desktop-integration.record"
            ),
            "CODEX_PLUS_NATIVE_ENTRYPOINT_SILENT_INSTALLED": "1",
            "CODEX_PLUS_NATIVE_ENTRYPOINT_MANAGEMENT_INSTALLED": "0",
            "CODEX_PLUS_NATIVE_CODEX_LAUNCH_RECORD_PATH": str(
                root / "codex-launch.record"
            ),
            "CODEX_PLUS_NATIVE_UPDATE_METADATA_PATH": str(update_metadata),
            "CODEX_PLUS_NATIVE_UPDATE_ASSET_PATH": str(update_asset),
            "CODEX_PLUS_NATIVE_UPDATE_LAUNCH_RECORD_PATH": str(
                root / "update-launch.record"
            ),
            "CODEX_PLUS_NATIVE_UPDATE_CHECK_RECORD_PATH": str(
                root / "update-check.record"
            ),
            "CODEX_PLUS_NATIVE_PERF_REPORT": str(report),
            "CODEX_PLUS_NATIVE_PERF_EXIT_AFTER_MS": str(NATIVE_EXIT_AFTER_MS),
            "CODEX_PLUS_NATIVE_ENV_PROCESS_ONLY": "1",
        }
    )
    return environment, state / "manager-instance-endpoint.json", pending


def start_manager(
    binary: Path,
    arguments: list[str],
    environment: dict[str, str],
    working_directory: Path,
) -> subprocess.Popen[bytes]:
    try:
        return subprocess.Popen(
            [str(binary), *arguments],
            cwd=working_directory,
            env=environment,
            **process_options(),
        )
    except OSError:
        fail("manager process could not be started")


def run_native(binary: Path, mode: str, root: Path) -> None:
    environment, endpoint, pending = native_environment(root, available_port(), mode)
    primary = start_manager(binary, [], environment, root)
    try:
        wait_for_file(endpoint, primary, 25)
        if mode != "ordinary":
            argument = "--show-update" if mode == "show-update" else provider_url()
            secondary = start_manager(binary, [argument], environment, root)
            try:
                secondary_code = wait_for_exit(secondary, 10)
            finally:
                stop_process(secondary)
            if secondary_code != 0:
                fail(f"secondary manager exited with code {secondary_code}")
            if mode == "provider-import":
                wait_for_file(pending, primary, 5)
                try:
                    request = json.loads(pending.read_text(encoding="utf-8"))
                except (OSError, json.JSONDecodeError):
                    fail("provider import did not produce a valid pending request")
                if request.get("name") != "Package Fixture Provider":
                    fail("provider import pending request identity is invalid")
        primary_code = wait_for_exit(primary, 25)
        if primary_code != 0:
            detail = bounded_native_failure(root, primary.pid)
            fail(f"primary manager exited with code {primary_code}: {detail}")
        if endpoint.exists():
            fail("manager instance endpoint remained after explicit exit")
        if not (root / f"perf-{mode}.json").is_file():
            fail("Native manager did not write its bounded launch report")
    finally:
        stop_process(primary)


def disposable_profile_state() -> Path:
    if os.name != "nt":
        fail("disposable Windows profile was requested on another platform")
    if os.environ.get("CODEX_PLUS_PACKAGE_DISPOSABLE_PROFILE") != "1":
        fail("previous Windows package probes require a disposable runner profile")
    if os.environ.get("GITHUB_ACTIONS") == "true":
        configured_profile = os.environ.get("CODEX_PLUS_PACKAGE_WINDOWS_PROFILE", "")
        if not configured_profile:
            fail("GitHub package probes require the Windows Known Folder profile")
        profile = Path(configured_profile)
        if not profile.is_absolute():
            fail("Windows Known Folder profile must be absolute")
    else:
        profile = Path.home()
    state = profile / ".codex-session-delete"
    marker = state / PROFILE_MARKER
    if not marker.is_file():
        fail("disposable runner profile is missing its ownership marker")
    return state


def run_previous(binary: Path, mode: str, root: Path) -> None:
    environment = isolated_environment(root, available_port())
    profile_state = disposable_profile_state() if os.name == "nt" else None
    state_directories = previous_state_directories(root, profile_state, binary.parent)
    pending_before = {
        state / "pending-provider-import.json": file_signature(
            state / "pending-provider-import.json"
        )
        for state in state_directories
    }
    diagnostic_offsets = {
        state / "codex-plus.log": file_size(state / "codex-plus.log")
        for state in state_directories
    }
    arguments = []
    if mode == "show-update":
        arguments.append("--show-update")
    elif mode == "provider-import":
        arguments.append(provider_url())
    process = start_manager(binary, arguments, environment, root)
    try:
        deadline = time.monotonic() + 4
        while time.monotonic() < deadline:
            code = process.poll()
            if code is not None:
                fail(f"previous manager exited during launch with code {code}")
            time.sleep(0.05)
        if mode == "provider-import":
            accepted, detail = provider_import_evidence(
                state_directories,
                pending_before,
                diagnostic_offsets,
                process.pid,
            )
            if not accepted:
                fail(f"previous manager did not accept the provider import launch: {detail}")
    finally:
        stop_process(process)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--mode", required=True, choices=MODES)
    parser.add_argument("--fixture-root", required=True)
    parser.add_argument("--version", required=True)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    binary = Path(args.binary).expanduser().resolve()
    fixture_root = Path(args.fixture_root).expanduser().resolve()
    if binary.is_symlink() or not binary.is_file():
        fail("stable manager binary must be a regular file")
    if not (fixture_root / MARKER).is_file():
        fail("launch probe requires a marked package fixture root")
    if len(args.version) > 128:
        fail("package version label exceeds the probe bound")
    expected_name = "codex-plus-plus-manager.exe" if os.name == "nt" else "CodexPlusPlusManager"
    if binary.name != expected_name:
        fail("manager binary does not use the stable installed filename")

    probes_root = fixture_root / "probes"
    probes_root.mkdir(exist_ok=True)
    implementation = "native" if args.version == "native" else "previous"
    run_root = Path(tempfile.mkdtemp(prefix=f"{implementation}-{args.mode}-", dir=probes_root))
    if implementation == "native":
        run_native(binary, args.mode, run_root)
    else:
        run_previous(binary, args.mode, run_root)
    print(json.dumps({"implementation": implementation, "mode": args.mode, "result": "passed"}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
