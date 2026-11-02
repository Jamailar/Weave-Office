#!/usr/bin/env python3
"""Package a verified macOS app as a drag-to-Applications disk image."""

from __future__ import annotations

import argparse
from pathlib import Path
import plistlib
import subprocess
import tempfile


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    app = args.app.resolve(strict=True)
    output = args.output.resolve()
    if output.exists() or output.suffix != ".dmg":
        parser.error("--output must be a new .dmg path")
    info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
    if info.get("CFBundleIdentifier") != "local.weaveoffice.desktop":
        parser.error("not a Weave Office application bundle")
    subprocess.run(["codesign", "--verify", "--deep", "--strict", str(app)], check=True)
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".weave-dmg-", dir=output.parent) as temporary:
        root = Path(temporary)
        stage = root / "stage"
        stage.mkdir()
        subprocess.run(["ditto", str(app), str(stage / app.name)], check=True)
        (stage / "Applications").symlink_to("/Applications", target_is_directory=True)
        image = root / output.name
        subprocess.run([
            "hdiutil", "create", "-volname", f"Weave Office {info['CFBundleShortVersionString']}",
            "-fs", "HFS+", "-srcfolder", str(stage), "-format", "UDZO", str(image),
        ], check=True)
        subprocess.run(["hdiutil", "verify", str(image)], check=True)
        image.rename(output)
    print(output)


if __name__ == "__main__":
    main()
