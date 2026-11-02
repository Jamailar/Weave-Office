#!/usr/bin/env python3
"""Run build, test or run only when Cargo targets the mounted external volume."""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys

from external_cargo import ROOT, build_environment, cargo_metadata


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("build", "test", "run"))
    parser.add_argument("cargo_args", nargs=argparse.REMAINDER, help="arguments passed to Cargo")
    args = parser.parse_args()
    if shutil.which("cargo") is None:
        parser.error("cargo is required")
    try:
        cargo_metadata()
        env = build_environment()
        command = ["cargo", args.command, *args.cargo_args]
        return subprocess.run(command, cwd=ROOT, env=env, check=False).returncode
    except (OSError, RuntimeError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
