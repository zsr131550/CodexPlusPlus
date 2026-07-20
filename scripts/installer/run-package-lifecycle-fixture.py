#!/usr/bin/env python3
"""Exercise a pinned package -> Native -> pinned package lifecycle in a fixture root."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import stat
import subprocess
import sys
from typing import NoReturn
import zipfile


MARKER = ".codex-plus-package-fixture"
MAX_FILES = 4096
MAX_TOTAL_BYTES = 1024 * 1024 * 1024
MODES = ("ordinary", "show-update", "provider-import")
CANARIES = {
    "business/settings.json": b'{"package_fixture_canary":"settings-v1"}\n',
    "scripts/user-script.js": b'console.log("package-fixture-canary");\n',
    "sessions/session.jsonl": b'{"fixture":"session-v1"}\n',
    "context/config.toml": b'[mcp_servers.fixture]\ncommand = "fixture"\n',
    "ownership/context-live-ownership.json": b'{"schema":1,"entries":[]}\n',
    "preferences/legacy-native.ron": b'(fixture_canary:"preferences-v1")\n',
    "watcher.disabled": b'disabled\n',
}


def fail(message: str) -> NoReturn:
    raise SystemExit(f"error: {message}")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def validate_digest(value: str) -> str:
    normalized = value.strip().lower()
    if len(normalized) != 64 or any(character not in "0123456789abcdef" for character in normalized):
        fail("previous SHA-256 must contain exactly 64 lowercase hexadecimal characters")
    return normalized


def create_fixture_root(value: str) -> Path:
    root = Path(value).expanduser()
    if root.exists() or root.is_symlink():
        fail("fixture root must not already exist")
    resolved_parent = root.parent.resolve()
    if resolved_parent == Path.home().resolve():
        fail("fixture root must not be a direct child of the user home")
    root.mkdir(parents=True)
    resolved = root.resolve()
    (resolved / MARKER).write_text("schema=1\n", encoding="ascii", newline="\n")
    return resolved


def safe_archive_path(name: str) -> Path:
    if "\\" in name or ":" in name:
        fail(f"unsafe package entry: {name}")
    value = PurePosixPath(name)
    if value.is_absolute() or not value.parts or any(part in ("", ".", "..") for part in value.parts):
        fail(f"unsafe package entry: {name}")
    return Path(*value.parts)


def extract_pinned_zip(package: Path, destination: Path) -> None:
    count = 0
    total = 0
    with zipfile.ZipFile(package) as archive:
        for entry in archive.infolist():
            relative = safe_archive_path(entry.filename)
            mode = entry.external_attr >> 16
            if stat.S_ISLNK(mode):
                fail(f"pinned package contains a symlink: {entry.filename}")
            target = destination / relative
            if entry.is_dir():
                target.mkdir(parents=True, exist_ok=True)
                continue
            count += 1
            total += entry.file_size
            if count > MAX_FILES or total > MAX_TOTAL_BYTES:
                fail("pinned package exceeds fixture extraction bounds")
            target.parent.mkdir(parents=True, exist_ok=True)
            with archive.open(entry) as source, target.open("xb") as output:
                shutil.copyfileobj(source, output, length=1024 * 1024)
            if mode:
                target.chmod(stat.S_IMODE(mode))


def relative_files(root: Path) -> list[str]:
    files: list[str] = []
    for path in sorted(root.rglob("*"), key=lambda candidate: candidate.as_posix()):
        if path.is_symlink():
            fail(f"fixture package trees must not contain symlinks: {path.name}")
        if path.is_file():
            files.append(path.relative_to(root).as_posix())
    return files


def copy_package(source: Path, install_root: Path) -> list[str]:
    owned = relative_files(source)
    for relative in owned:
        source_path = source / relative
        target = install_root / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source_path, target)
    return owned


def remove_owned(install_root: Path, owned: list[str]) -> None:
    for relative in sorted(set(owned), reverse=True):
        target = install_root / relative
        if target.is_symlink():
            fail(f"refusing to remove a linked owned path: {relative}")
        try:
            target.unlink()
        except FileNotFoundError:
            pass
    directories = sorted(
        (path for path in install_root.rglob("*") if path.is_dir()),
        key=lambda path: len(path.parts),
        reverse=True,
    )
    for directory in directories:
        try:
            directory.rmdir()
        except OSError:
            pass


def assert_install_root_empty(install_root: Path) -> None:
    if any(install_root.rglob("*")):
        fail("package uninstall left package-owned directories or links")


def seed_canaries(data_root: Path) -> dict[str, object]:
    for relative, contents in CANARIES.items():
        target = data_root / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(contents)
    return tree_summary(data_root)


def tree_summary(root: Path) -> dict[str, object]:
    aggregate = hashlib.sha256()
    entries: list[dict[str, object]] = []
    for relative in relative_files(root):
        path = root / relative
        digest = sha256(path)
        size = path.stat().st_size
        aggregate.update(relative.encode("utf-8"))
        aggregate.update(b"\0")
        aggregate.update(digest.encode("ascii"))
        aggregate.update(b"\0")
        entries.append({"path": relative, "size": size, "sha256": digest})
    return {"file_count": len(entries), "sha256": aggregate.hexdigest(), "files": entries}


def manager_path(install_root: Path, platform: str) -> Path:
    if platform == "windows-x64":
        candidates = list(install_root.rglob("codex-plus-plus-manager.exe"))
    else:
        candidates = [
            path
            for path in install_root.rglob("*")
            if path.is_file() and path.name in ("CodexPlusPlusManager", "codex-plus-plus-manager")
        ]
    if len(candidates) != 1:
        fail(f"installed fixture must contain exactly one stable manager, found {len(candidates)}")
    return candidates[0]


def load_native_manifest(path: Path, platform: str) -> dict[str, object]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot load Native package manifest: {error}")
    if value.get("schema") != 1 or value.get("implementation") != "native":
        fail("Native package manifest identity is invalid")
    if value.get("platform") != platform:
        fail("Native package manifest platform does not match the fixture")
    staged = value.get("staged")
    manager = value.get("manager")
    if not isinstance(staged, dict) or not isinstance(manager, dict):
        fail("Native package manifest is missing manager hashes")
    for key in ("source_sha256", "staged_sha256"):
        validate_digest(str(manager.get(key, "")))
    return value


def manifest_relative_path(value: object) -> str:
    if not isinstance(value, str) or "\\" in value or ":" in value:
        fail("Native package manifest contains an unsafe path")
    relative = safe_archive_path(value).as_posix()
    if relative != value:
        fail("Native package manifest paths must use normalized forward slashes")
    return relative


def verify_native_tree(
    root: Path, manifest: dict[str, object]
) -> dict[str, dict[str, object]]:
    entries = manifest.get("files")
    if not isinstance(entries, list) or not entries or len(entries) > MAX_FILES:
        fail("Native package manifest file list is invalid")

    expected: dict[str, dict[str, object]] = {}
    for entry in entries:
        if not isinstance(entry, dict):
            fail("Native package manifest file entry is invalid")
        relative = manifest_relative_path(entry.get("path"))
        if relative in expected:
            fail("Native package manifest contains duplicate paths")
        try:
            size = int(entry.get("size"))
        except (TypeError, ValueError):
            fail("Native package manifest file size is invalid")
        if size < 0 or size > MAX_TOTAL_BYTES:
            fail("Native package manifest file size exceeds fixture bounds")
        digest = validate_digest(str(entry.get("sha256", "")))
        expected[relative] = {"size": size, "sha256": digest}

    actual = relative_files(root)
    if actual != sorted(expected):
        fail("Native package tree does not exactly match its manifest")
    for relative, entry in expected.items():
        path = root / relative
        if path.stat().st_size != entry["size"] or sha256(path) != entry["sha256"]:
            fail("Native package file does not match its manifest hash")

    staged = manifest.get("staged")
    manager = manifest.get("manager")
    if not isinstance(staged, dict) or not isinstance(manager, dict):
        fail("Native package manifest is missing staged manager identity")
    staged_relative = manifest_relative_path(staged.get("path"))
    staged_entry = expected.get(staged_relative)
    if staged_entry is None:
        fail("Native package manifest does not enumerate the stable manager")
    staged_digest = validate_digest(str(staged.get("sha256", "")))
    manager_digest = validate_digest(str(manager.get("staged_sha256", "")))
    if staged_entry["sha256"] != staged_digest or staged_digest != manager_digest:
        fail("Native stable manager hashes are inconsistent")
    return expected


def probe_command(probe: Path, binary: Path, mode: str, fixture_root: Path, version: str) -> list[str]:
    prefix = [sys.executable, str(probe)] if probe.suffix.lower() == ".py" else [str(probe)]
    return prefix + [
        "--binary",
        str(binary),
        "--mode",
        mode,
        "--fixture-root",
        str(fixture_root),
        "--version",
        version,
    ]


def run_probes(probe: Path, binary: Path, fixture_root: Path, version: str) -> list[dict[str, object]]:
    outcomes: list[dict[str, object]] = []
    for mode in MODES:
        try:
            result = subprocess.run(
                probe_command(probe, binary, mode, fixture_root, version),
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                timeout=90,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired):
            fail(f"{version} {mode} launch probe could not complete")
        if result.returncode != 0:
            fail(f"{version} {mode} launch probe failed with exit code {result.returncode}")
        outcomes.append({"mode": mode, "exit_code": result.returncode})
    return outcomes


def assert_data_unchanged(data_root: Path, baseline: dict[str, object]) -> dict[str, object]:
    current = tree_summary(data_root)
    if current != baseline:
        fail("business data canaries changed during package lifecycle")
    return current


def main() -> int:
    args = parse_args()
    package = Path(args.previous_package).expanduser().resolve()
    native_root = Path(args.native_package_root).expanduser().resolve()
    native_manifest_path = Path(args.native_manifest).expanduser().resolve()
    probe = Path(args.probe).expanduser().resolve()
    if package.suffix.lower() != ".zip" or not package.is_file():
        fail("previous package must be an explicitly supplied zip asset")
    if not native_root.is_dir() or not native_manifest_path.is_file() or not probe.is_file():
        fail("Native package root, manifest, and launch probe must exist")

    expected_previous_hash = validate_digest(args.previous_sha256)
    actual_previous_hash = sha256(package)
    if actual_previous_hash != expected_previous_hash:
        fail("previous package SHA-256 does not match the pinned value")

    native_manifest = load_native_manifest(native_manifest_path, args.platform)
    verify_native_tree(native_root, native_manifest)
    fixture_root = create_fixture_root(args.fixture_root)
    previous_stage = fixture_root / "previous-stage"
    install_root = fixture_root / "installed"
    data_root = fixture_root / "data"
    previous_stage.mkdir()
    install_root.mkdir()
    extract_pinned_zip(package, previous_stage)
    baseline = seed_canaries(data_root)

    previous_owned = copy_package(previous_stage, install_root)
    previous_launches = run_probes(
        probe,
        manager_path(install_root, args.platform),
        fixture_root,
        args.previous_version,
    )
    assert_data_unchanged(data_root, baseline)

    remove_owned(install_root, previous_owned)
    native_owned = copy_package(native_root, install_root)
    native_launches = run_probes(
        probe,
        manager_path(install_root, args.platform),
        fixture_root,
        "native",
    )
    assert_data_unchanged(data_root, baseline)

    remove_owned(install_root, native_owned)
    assert_install_root_empty(install_root)
    assert_data_unchanged(data_root, baseline)

    restored_owned = copy_package(previous_stage, install_root)
    restored_launches = run_probes(
        probe,
        manager_path(install_root, args.platform),
        fixture_root,
        args.previous_version,
    )
    final_data = assert_data_unchanged(data_root, baseline)

    result = {
        "schema": 1,
        "platform": args.platform,
        "previous": {
            "version": args.previous_version,
            "sha256": actual_previous_hash,
            "owned_file_count": len(previous_owned),
            "restored_file_count": len(restored_owned),
            "launches": previous_launches,
            "restored_launches": restored_launches,
        },
        "native": {
            "source_sha256": native_manifest["manager"]["source_sha256"],
            "staged_sha256": native_manifest["manager"]["staged_sha256"],
            "owned_file_count": len(native_owned),
            "launches": native_launches,
            "uninstall_clean": True,
        },
        "data": {
            "file_count": final_data["file_count"],
            "sha256": final_data["sha256"],
            "unchanged": True,
        },
    }
    output = Path(args.output).expanduser().resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_name(f".{output.name}.tmp-{os.getpid()}")
    try:
        temporary.write_text(
            json.dumps(result, ensure_ascii=True, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
            newline="\n",
        )
        os.replace(temporary, output)
    finally:
        try:
            temporary.unlink()
        except FileNotFoundError:
            pass
    print(json.dumps({"result": "passed", "platform": args.platform}, sort_keys=True))
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixture-root", required=True)
    parser.add_argument("--previous-package", required=True)
    parser.add_argument("--previous-version", required=True)
    parser.add_argument("--previous-sha256", required=True)
    parser.add_argument("--native-package-root", required=True)
    parser.add_argument("--native-manifest", required=True)
    parser.add_argument("--platform", required=True, choices=("windows-x64", "macos-x64", "macos-arm64"))
    parser.add_argument("--probe", required=True)
    parser.add_argument("--output", required=True)
    return parser.parse_args()


if __name__ == "__main__":
    sys.exit(main())
