#!/usr/bin/env python3
"""Prepare the pinned, external translation catalogs used by the hosted Word editor."""
from __future__ import annotations

import argparse
from pathlib import Path
import re
import subprocess
import sys
import tempfile

from weave_volume import dev_volume

ROOT = Path(__file__).resolve().parent.parent
VOLUME = dev_volume()
DEFAULT_DIR = VOLUME / "DevCache/WeaveOffice/inputs/craft-i18n"
REPOSITORY = "https://github.com/storytold/craft-i18n"


def git(*args: str, cwd: Path | None = None) -> str:
    return subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True, text=True).stdout.strip()


def pinned_revision() -> str:
    revision = (ROOT / "vendor/wordcraft/craft-i18n.rev").read_text().strip()
    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise ValueError("invalid pinned WordCraft translation revision")
    return revision


def verify(directory: Path) -> None:
    directory = directory.resolve(strict=True)
    if git("rev-parse", "HEAD", cwd=directory) != pinned_revision():
        raise ValueError("translation checkout does not match vendor/wordcraft/craft-i18n.rev")
    if git("status", "--porcelain", cwd=directory):
        raise ValueError("translation checkout has local changes; use a clean pinned checkout")
    for name in ("LICENSE-MIT", "LICENSE-APACHE", "ATTRIBUTION.md"):
        if not (directory / name).is_file():
            raise ValueError(f"missing translation attribution: {name}")
    rows = 0
    for catalog in ("common.tsv", "wordcraft.tsv"):
        path = directory / "locales/zh-hans" / catalog
        seen = set()
        active = 0
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            if not line.strip() or line.lstrip().startswith("#"):
                continue
            fields = line.split("\t")
            if len(fields) != 3 or not fields[1] or not fields[2]:
                raise ValueError(f"{catalog}:{number}: malformed translation row")
            key = tuple(fields[:2])
            if key in seen:
                raise ValueError(f"{catalog}:{number}: duplicate translation key")
            seen.add(key)
            active += 1
        if not active:
            raise ValueError(f"empty Simplified Chinese catalog: {catalog}")
        rows += active
    print(f"Verified Simplified Chinese catalogs ({rows} rows) at {pinned_revision()}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dir", type=Path, default=DEFAULT_DIR)
    parser.add_argument("--verify-only", action="store_true")
    args = parser.parse_args()
    if not VOLUME.is_mount():
        parser.error(f"mount {VOLUME} (WEAVE_DEV_VOLUME) before preparing translations")
    directory = args.dir.expanduser().resolve()
    if not directory.is_relative_to(VOLUME.resolve(strict=True)) or directory.is_relative_to(ROOT):
        parser.error("translation inputs must stay on the mounted external volume, outside the source tree")
    try:
        if not directory.exists() and not args.verify_only:
            directory.parent.mkdir(parents=True, exist_ok=True)
            with tempfile.TemporaryDirectory(prefix="craft-i18n-", dir=directory.parent) as temporary:
                checkout = Path(temporary) / "checkout"
                git("clone", "--filter=blob:none", "--no-checkout", REPOSITORY, str(checkout))
                git("checkout", "--detach", pinned_revision(), cwd=checkout)
                verify(checkout)
                checkout.rename(directory)
        verify(directory)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"translation preparation failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
