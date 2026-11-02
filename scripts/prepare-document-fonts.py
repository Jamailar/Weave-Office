#!/usr/bin/env python3
"""Fetch and verify the pinned Simplified Chinese document serif input."""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import shutil
import sys
import tempfile
from urllib.request import urlopen

from weave_volume import dev_volume


REV = "f8d157532fbfaeda587e826d4cd5b21a49186f7c"
DEFAULT_DIR = dev_volume() / "DevCache/WeaveOffice/inputs/weave-document-fonts"
ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "scripts/document-fonts-manifest.txt"
FONT = "NotoSerifCJKsc-Regular.otf"
FONT_SHA256 = "2a2eae2628df83556c54018c41e20fa532c1b862c5256ae8b3f23feb918d12ca"
LICENSE = "OFL.txt"
LICENSE_SHA256 = "6a73f9541c2de74158c0e7cf6b0a58ef774f5a780bf191f2d7ec9cc53efe2bf2"
SOURCE = f"https://raw.githubusercontent.com/notofonts/noto-cjk/{REV}/Serif"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def verify(directory: Path) -> None:
    directory = directory.resolve(strict=True)
    if directory.is_relative_to(ROOT):
        raise ValueError("document fonts must stay outside this repository")
    manifest = directory / "manifest.txt"
    if not manifest.is_file() or manifest.read_bytes() != MANIFEST.read_bytes():
        raise ValueError("document font manifest differs from the pinned project manifest")
    for name, expected in ((FONT, FONT_SHA256), (LICENSE, LICENSE_SHA256)):
        path = (directory / name).resolve(strict=True)
        if path.parent != directory or not path.is_file():
            raise ValueError(f"invalid document font input: {name}")
        actual = sha256(path)
        if actual != expected:
            raise ValueError(f"{name}: SHA-256 mismatch: {actual}")
    if "SIL OPEN FONT LICENSE Version 1.1" not in (directory / LICENSE).read_text(encoding="utf-8"):
        raise ValueError("missing OFL-1.1 license text")
    print(f"Verified Noto Serif CJK SC Regular at {REV} ({(directory / FONT).stat().st_size:,} bytes)")
    print(f"WEAVE_DOCUMENT_FONTS_DIR={directory}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dir", type=Path, default=DEFAULT_DIR)
    parser.add_argument("--verify-only", action="store_true")
    args = parser.parse_args()
    target = args.dir.expanduser().resolve()
    volume = dev_volume()
    if not volume.is_mount():
        parser.error(f"{volume} is not mounted")
    if not target.is_relative_to(volume.resolve(strict=True)) or target.is_relative_to(ROOT):
        parser.error("--dir must be outside this repository on the mounted external volume")
    try:
        if not target.exists() and not args.verify_only:
            target.parent.mkdir(parents=True, exist_ok=True)
            with tempfile.TemporaryDirectory(prefix="weave-document-fonts-", dir=target.parent) as temp:
                staging = Path(temp) / "input"
                staging.mkdir()
                shutil.copy2(MANIFEST, staging / "manifest.txt")
                for name, source in ((FONT, f"{SOURCE}/OTF/SimplifiedChinese/{FONT}"), (LICENSE, f"{SOURCE}/LICENSE")):
                    with urlopen(source, timeout=60) as response, (staging / name).open("wb") as output:
                        shutil.copyfileobj(response, output)
                verify(staging)
                staging.rename(target)
        verify(target)
    except (OSError, ValueError) as error:
        print(f"document font preparation failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
