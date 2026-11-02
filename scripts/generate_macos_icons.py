#!/usr/bin/env python3
"""Generate original macOS icon resources with intermediates on Jamba only."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import subprocess
import tempfile

from external_cargo import ROOT, VOLUME, require_volume


ICON_FILES = {
    "app": "WeaveOffice.icns",
    "word": "WeaveWord.icns",
    "sheet": "WeaveSheet.icns",
    "slides": "WeaveSlides.icns",
    "pdf": "WeavePDF.icns",
}
CACHE = VOLUME / "DevCache" / "WeaveOffice" / "macos-icons"


def generate(destination: Path) -> None:
    require_volume()
    cache_root = CACHE.resolve(strict=False)
    if not cache_root.is_relative_to(VOLUME.resolve(strict=True)):
        raise RuntimeError(f"icon cache resolves outside Jamba: {cache_root}")
    cache_root.mkdir(parents=True, exist_ok=True)
    destination.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="generate-", dir=cache_root) as temporary:
        work = Path(temporary)
        module_cache = work / "swift-module-cache"
        module_cache.mkdir()
        generator = work / "macos-icons"
        subprocess.run(
            ["swiftc", "-module-cache-path", str(module_cache),
             str(ROOT / "scripts" / "macos-icons.swift"), "-o", str(generator)],
            check=True,
            env={**os.environ,
                 "TMPDIR": str(work), "TMP": str(work), "TEMP": str(work),
                 "SWIFT_MODULECACHE_PATH": str(module_cache),
                 "CLANG_MODULE_CACHE_PATH": str(module_cache)},
        )
        for kind, filename in ICON_FILES.items():
            iconset = work / f"{kind}.iconset"
            subprocess.run([str(generator), kind, str(iconset),
                            *([str(ROOT / "assets/app-icon/weave-office.png")] if kind == "app" else [])], check=True,
                           env={**os.environ, "TMPDIR": str(work), "TMP": str(work), "TEMP": str(work)})
            subprocess.run(["iconutil", "-c", "icns", str(iconset),
                            "-o", str(destination / filename)], check=True,
                           env={**os.environ, "TMPDIR": str(work), "TMP": str(work), "TEMP": str(work)})


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path, help="final Resources directory")
    args = parser.parse_args()
    generate(args.destination)


if __name__ == "__main__":
    main()
