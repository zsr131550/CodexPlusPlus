#!/usr/bin/env python3
"""Write a bounded, path-safe manifest for one staged package tree.

The manifest deliberately records relative paths and content hashes only.  The
source binary is outside the staged tree, so its absolute path is never emitted.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import sys
from typing import Iterable, NoReturn


MAX_FILES = 4096
MAX_FILE_BYTES = 512 * 1024 * 1024
MAX_TOTAL_BYTES = 1024 * 1024 * 1024
CHUNK_BYTES = 1024 * 1024


def fail(message: str) -> "NoReturn":
    raise SystemExit(f"error: {message}")


def resolve_root(value: str) -> Path:
    root = Path(value).expanduser().resolve()
    if not root.is_dir():
        fail(f"package root is not a directory: {value}")
    return root


def resolve_source(value: str) -> Path:
    candidate = Path(value).expanduser()
    if candidate.is_symlink():
        fail(f"source binary must not be a symlink: {value}")
    source = candidate.resolve()
    if not source.is_file():
        fail(f"source binary is not a regular file: {value}")
    return source


def relative_to(root: Path, path: Path) -> str:
    try:
        relative = path.relative_to(root)
    except ValueError:
        fail(f"path escapes package root: {path}")
    return relative.as_posix()


def sha256(path: Path) -> tuple[str, int]:
    digest = hashlib.sha256()
    size = 0
    try:
        with path.open("rb") as stream:
            while chunk := stream.read(CHUNK_BYTES):
                size += len(chunk)
                if size > MAX_FILE_BYTES:
                    fail(f"file exceeds bounded manifest size: {path.name}")
                digest.update(chunk)
    except OSError as error:
        fail(f"cannot hash {path.name}: {error}")
    return digest.hexdigest(), size


def staged_files(root: Path, output: Path) -> Iterable[tuple[str, Path]]:
    count = 0
    total = 0
    for path in sorted(root.rglob("*"), key=lambda candidate: candidate.as_posix()):
        if path.is_symlink():
            relative = relative_to(root, path)
            if relative == "Applications" and os.readlink(path) == "/Applications":
                continue
            fail(f"symlinks are not allowed in a package tree: {path.name}")
        if not path.is_file():
            continue
        if path.resolve() == output:
            continue
        count += 1
        if count > MAX_FILES:
            fail(f"package contains more than {MAX_FILES} files")
        total += path.stat().st_size
        if total > MAX_TOTAL_BYTES:
            fail(f"package exceeds {MAX_TOTAL_BYTES} total bytes")
        yield relative_to(root, path), path


def build_manifest(args: argparse.Namespace) -> dict[str, object]:
    root = resolve_root(args.root)
    source = resolve_source(args.source_binary)
    output = Path(args.output).expanduser().resolve()
    staged_relative = Path(args.staged_binary)
    if staged_relative.is_absolute() or ".." in staged_relative.parts:
        fail("staged binary must be a relative path inside the package root")
    staged = root / staged_relative
    if staged.is_symlink() or not staged.is_file():
        fail(f"staged binary is not a regular file: {args.staged_binary}")

    source_hash, source_size = sha256(source)
    staged_hash, staged_size = sha256(staged)
    bytes_match = source_hash == staged_hash and source_size == staged_size
    if not bytes_match and not args.allow_packaging_transform:
        fail("staged manager bytes do not match the Native source binary")

    entries: list[dict[str, object]] = []
    forbidden = tuple(token.lower() for token in args.forbid)
    for relative, path in staged_files(root, output):
        if any(token and token in relative.lower() for token in forbidden):
            fail(f"forbidden implementation marker in staged path: {relative}")
        digest, size = sha256(path)
        entries.append({"path": relative, "size": size, "sha256": digest})

    manager_entries = [entry for entry in entries if entry["path"] == staged_relative.as_posix()]
    if len(manager_entries) != 1:
        fail("package must contain exactly one stable manager binary")

    return {
        "schema": 1,
        "platform": args.platform,
        "implementation": "native",
        "stable_identity": {
            "binary": "codex-plus-plus-manager",
            "display": "Codex++ Manager",
        },
        "manager": {
            "source_sha256": source_hash,
            "staged_sha256": staged_hash,
            "bytes_match": bytes_match,
            "packaging_transform": not bytes_match,
        },
        "source": {"name": source.name, "size": source_size, "sha256": source_hash},
        "staged": {
            "path": staged_relative.as_posix(),
            "size": staged_size,
            "sha256": staged_hash,
        },
        "links": ([{"path": "Applications", "target": "/Applications"}]
                  if (root / "Applications").is_symlink() else []),
        "files": entries,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True, help="staged package root")
    parser.add_argument("--output", required=True, help="manifest output path")
    parser.add_argument("--platform", required=True, help="bounded platform label")
    parser.add_argument("--source-binary", required=True, help="Native source binary")
    parser.add_argument("--staged-binary", required=True, help="relative staged manager path")
    parser.add_argument(
        "--forbid",
        action="append",
        default=[],
        help="case-insensitive path marker forbidden in the staged tree",
    )
    parser.add_argument(
        "--allow-packaging-transform",
        action="store_true",
        help="allow signing to change staged bytes while recording both hashes",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    manifest = build_manifest(args)
    output = Path(args.output).expanduser().resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_name(f".{output.name}.tmp-{os.getpid()}")
    try:
        with temporary.open("w", encoding="utf-8", newline="\n") as stream:
            json.dump(manifest, stream, ensure_ascii=True, indent=2, sort_keys=True)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, output)
    finally:
        try:
            temporary.unlink()
        except FileNotFoundError:
            pass
    print(json.dumps({"result": "written", "platform": args.platform}, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
