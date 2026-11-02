#!/usr/bin/env python3
"""Fetch and verify the pinned, external craft-fonts build input."""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import re
import subprocess
import sys
import tempfile

from weave_volume import dev_volume


REPO = "https://github.com/storytold/craft-fonts"
REV = "37827adc41fd165f9a9234ac40f94dbad353aa69"
DEFAULT_DIR = dev_volume() / "DevCache/WeaveOffice/inputs/craft-fonts"
PROJECT_ROOT = Path(__file__).resolve().parent.parent
MANIFESTS = (
    "fonts/manifest.txt",
    "fonts/simplified-chinese-manifest.txt",
    "fonts/latin-manifest.txt",
)


def git(*args: str, cwd: Path | None = None) -> str:
    result = subprocess.run(["git", *args], cwd=cwd, check=True, text=True, capture_output=True)
    return result.stdout.strip()


def checked_file(root: Path, relative: str) -> Path:
    path = (root / relative).resolve(strict=True)
    if not path.is_relative_to(root) or not path.is_file():
        raise ValueError(f"file outside checkout or not regular: {relative}")
    return path


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def verify(root: Path, *, report: bool = True) -> None:
    root = root.resolve(strict=True)
    if root.is_relative_to(PROJECT_ROOT):
        raise ValueError("craft-fonts must stay outside this repository")
    actual = git("rev-parse", "HEAD", cwd=root)
    if actual != REV:
        raise ValueError(f"unexpected craft-fonts revision: {actual}; expected {REV}")
    if git("status", "--porcelain", cwd=root):
        raise ValueError("craft-fonts checkout has local changes; use a clean pinned checkout")

    count = 0
    total = 0
    seen = set()
    faces: dict[str, set[tuple[str, str]]] = {}
    for name in MANIFESTS:
        manifest = checked_file(root, name)
        manifest_faces: set[tuple[str, str]] = set()
        for number, line in enumerate(manifest.read_text(encoding="utf-8").splitlines(), 1):
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            fields = [field.strip() for field in line.split(" | ")]
            if len(fields) != 8:
                raise ValueError(f"{name}:{number}: expected eight fields")
            family, style, font_name, scripts, licence, licence_name, expected, source = fields
            if not all((family, style, scripts)) or licence != "OFL-1.1" or not source.startswith("https://"):
                raise ValueError(f"{name}:{number}: incomplete or unapproved metadata")
            if not re.fullmatch(r"[0-9a-f]{64}", expected):
                raise ValueError(f"{name}:{number}: invalid SHA-256")
            if font_name in seen:
                raise ValueError(f"duplicate font file: {font_name}")
            seen.add(font_name)
            manifest_faces.add((family, style))
            font = checked_file(root, font_name)
            ofl = checked_file(root, licence_name)
            if ofl.parent != font.parent or ofl.name != "OFL.txt":
                raise ValueError(f"{name}:{number}: licence must be adjacent to font")
            if "SIL OPEN FONT LICENSE Version 1.1" not in ofl.read_text(encoding="utf-8"):
                raise ValueError(f"{name}:{number}: missing OFL-1.1 text")
            digest = sha256(font)
            if digest != expected:
                raise ValueError(f"{font_name}: SHA-256 mismatch: {digest}")
            count += 1
            total += font.stat().st_size
        if not manifest_faces:
            raise ValueError(f"empty font manifest: {name}")
        faces[name] = manifest_faces
    required = {
        "fonts/manifest.txt": {
            ("BIZ UDPGothic", "Regular"),
            ("BIZ UDPGothic", "Bold"),
            ("NanumGothic", "Regular"),
            ("Noto Sans Symbols", "Regular"),
            ("Noto Sans Symbols 2", "Regular"),
            ("Noto Sans CJK SC", "Regular"),
        },
        "fonts/simplified-chinese-manifest.txt": {("LXGW WenKai Lite", "Regular")},
        "fonts/latin-manifest.txt": {("Inter", "Regular"), ("Inter", "Bold")},
    }
    for name, expected in required.items():
        missing = expected - faces[name]
        if missing:
            raise ValueError(f"{name}: missing required faces: {sorted(missing)}")
    checked_file(root, "ATTRIBUTION.md")
    if report:
        details = ", ".join(f"{name}: {len(faces[name])}" for name in MANIFESTS)
        print(f"Verified {count} OFL-1.1 fonts ({total:,} bytes) at {REV} ({details})")
        print(f"CRAFT_FONTS_DIR={root}")
        print("CRAFT_FONTS_REQUIRED=1")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dir", type=Path, default=DEFAULT_DIR, help="external checkout directory")
    parser.add_argument("--verify-only", action="store_true", help="check an existing checkout without network")
    args = parser.parse_args()
    target = args.dir.expanduser().resolve()
    volume = dev_volume()
    if not volume.is_mount():
        parser.error(f"{volume} is not mounted; mount the external volume (WEAVE_DEV_VOLUME) before preparing fonts")
    if not target.is_relative_to(volume.resolve(strict=True)):
        parser.error("--dir must be on the mounted external volume")
    if target.is_relative_to(PROJECT_ROOT):
        parser.error("--dir must be outside this repository")
    try:
        if not target.exists() and not args.verify_only:
            target.parent.mkdir(parents=True, exist_ok=True)
            with tempfile.TemporaryDirectory(prefix="craft-fonts-", dir=target.parent) as temp:
                checkout = Path(temp) / "checkout"
                git("clone", "--filter=blob:none", "--no-checkout", REPO, str(checkout))
                git("checkout", "--detach", REV, cwd=checkout)
                verify(checkout, report=False)
                checkout.rename(target)
        verify(target)
    except (OSError, subprocess.CalledProcessError, ValueError) as exc:
        print(f"craft-fonts preparation failed: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
