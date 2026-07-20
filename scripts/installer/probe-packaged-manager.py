#!/usr/bin/env python3
"""Probe packaged manager launch modes without exposing fixture contents."""

from __future__ import annotations

import argparse
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


def isolated_environment(root: Path, port: int) -> dict[str, str]:
    user = root / "user"
    local = user / "local"
    roaming = user / "roaming"
    temp = root / "temp"
    for directory in (user, local, roaming, temp):
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


def start_manager(binary: Path, arguments: list[str], environment: dict[str, str]) -> subprocess.Popen[bytes]:
    try:
        return subprocess.Popen(
            [str(binary), *arguments],
            cwd=binary.parent,
            env=environment,
            **process_options(),
        )
    except OSError:
        fail("manager process could not be started")


def run_native(binary: Path, mode: str, root: Path) -> None:
    environment, endpoint, pending = native_environment(root, available_port(), mode)
    primary = start_manager(binary, [], environment)
    try:
        wait_for_file(endpoint, primary, 25)
        if mode != "ordinary":
            argument = "--show-update" if mode == "show-update" else provider_url()
            secondary = start_manager(binary, [argument], environment)
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
            fail(f"primary manager exited with code {primary_code}")
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
    state = Path.home() / ".codex-session-delete"
    marker = state / PROFILE_MARKER
    if state.exists() and not marker.is_file():
        fail("disposable runner profile already contains unmanaged application state")
    state.mkdir(parents=True, exist_ok=True)
    marker.write_text("schema=1\n", encoding="ascii", newline="\n")
    return state


def run_previous(binary: Path, mode: str, root: Path) -> None:
    environment = isolated_environment(root, available_port())
    profile_state = disposable_profile_state() if os.name == "nt" else None
    arguments = []
    if mode == "show-update":
        arguments.append("--show-update")
    elif mode == "provider-import":
        arguments.append(provider_url())
    process = start_manager(binary, arguments, environment)
    try:
        deadline = time.monotonic() + 4
        while time.monotonic() < deadline:
            code = process.poll()
            if code is not None:
                fail(f"previous manager exited during launch with code {code}")
            time.sleep(0.05)
        if mode == "provider-import":
            pending = (
                profile_state / "pending-provider-import.json"
                if profile_state is not None
                else root / "user" / ".codex-session-delete" / "pending-provider-import.json"
            )
            if not pending.is_file():
                fail("previous manager did not accept the provider import launch")
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
