#!/usr/bin/env python3
"""Cross-compile Windows MSVC releases on Linux and package per-user NSIS installers."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import runpy
import shutil
import shlex
import struct
import subprocess
import tempfile
import tomllib


ROOT = Path(__file__).resolve().parent.parent
TARGETS = {
    "x64": ("x86_64-pc-windows-msvc", 0x8664),
    "arm64": ("aarch64-pc-windows-msvc", 0xAA64),
    "x86": ("i686-pc-windows-msvc", 0x014C),
}


def verify_pe(executable: Path, machine: int) -> None:
    with executable.open("rb") as stream:
        if stream.read(2) != b"MZ":
            raise ValueError(f"not a Windows executable: {executable}")
        stream.seek(0x3C)
        stream.seek(struct.unpack("<I", stream.read(4))[0])
        if stream.read(4) != b"PE\0\0" or struct.unpack("<H", stream.read(2))[0] != machine:
            raise ValueError(f"incorrect Windows architecture: {executable}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inputs", type=Path, required=True, help="verified external font and translation inputs")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--target-dir", type=Path, required=True)
    parser.add_argument("--arch", choices=TARGETS, nargs="+", default=list(TARGETS))
    parser.add_argument("--offline", action="store_true")
    parser.add_argument("--jobs", type=int, default=3)
    parser.add_argument("--sign-command", default=os.environ.get("WEAVE_WINDOWS_SIGN_COMMAND"),
                        help='JSON signing command: {"cmd": "...", "args": ["...", "%%1"]}')
    args = parser.parse_args()
    signing = json.loads(args.sign_command) if args.sign_command else None
    if signing is not None and not (
        isinstance(signing, dict) and isinstance(signing.get("cmd"), str)
        and isinstance(signing.get("args"), list)
        and all(isinstance(arg, str) for arg in signing["args"])
        and any("%1" in arg for arg in signing["args"])
    ):
        parser.error("--sign-command must be a structured command with a %1 file argument")
    inputs = args.inputs.resolve(strict=True)
    output = args.output.resolve()
    target_dir = args.target_dir.resolve()
    if inputs.is_relative_to(ROOT) or target_dir.is_relative_to(ROOT):
        parser.error("inputs and Cargo output must stay outside the source checkout")
    # These modules use WEAVE_DEV_VOLUME to calculate their default paths. Verification below
    # receives explicit external paths, so a Linux builder can use its mounted root volume.
    os.environ.setdefault("WEAVE_DEV_VOLUME", "/")
    for script, name in (
        ("prepare-fonts.py", "craft-fonts"),
        ("prepare-document-fonts.py", "weave-document-fonts"),
        ("prepare-i18n.py", "craft-i18n"),
    ):
        runpy.run_path(str(ROOT / "scripts" / script))["verify"](inputs / name)

    # Reuse the macOS bundler's attribution logic for the exact same embedded inputs.
    bundle = runpy.run_path(str(ROOT / "scripts" / "build-macos-app.py"))
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
    env = os.environ.copy()
    env.update({
        "CARGO_TARGET_DIR": str(target_dir),
        "CRAFT_FONTS_DIR": str(inputs / "craft-fonts"),
        "CRAFT_FONTS_REQUIRED": "1",
        "WEAVE_DOCUMENT_FONTS_DIR": str(inputs / "weave-document-fonts"),
        "WEAVE_DOCUMENT_FONTS_REQUIRED": "1",
        "CRAFT_I18N_DIR": str(inputs / "craft-i18n"),
        "CRAFT_I18N_REQUIRED": "1",
        # The app icon and version info must be in the executable (build.rs, via llvm-rc).
        "WEAVE_REQUIRE_WINRES": "1",
        "XWIN_ARCH": ",".join({"x64": "x86_64", "arm64": "aarch64", "x86": "x86"}[arch]
                              for arch in args.arch),
        "RUSTFLAGS": (env.get("RUSTFLAGS", "") + " -C target-feature=+crt-static").strip(),
    })
    output.mkdir(parents=True, exist_ok=True)
    for arch in args.arch:
        triple, machine = TARGETS[arch]
        build_env = env.copy()
        if arch == "x86":
            # Clang 19 ignores cc-rs' -arch:SSE2 spelling; AWS-LC requires SSE2 on x86.
            # cc-rs merges hyphenated target flags with cargo-xwin's underscore SDK flags.
            # This leaves Linux host build dependencies on their own compiler flags.
            for name in ("CFLAGS_i686-pc-windows-msvc", "CXXFLAGS_i686-pc-windows-msvc"):
                build_env[name] = (build_env.get(name, "") + " /clang:-msse2").strip()
        command = ["cargo", "xwin", "build", "--locked", "--release", "--package", "weave-office",
                   "--bin", "weave-office", "--target", triple, "--jobs", str(args.jobs)]
        if args.offline:
            command.append("--offline")
        subprocess.run(command, cwd=ROOT, env=build_env, check=True)
        executable = target_dir / triple / "release" / "weave-office.exe"
        verify_pe(executable, machine)
        # Every load-time import must exist on every supported Windows, or the app cannot start.
        runpy.run_path(str(ROOT / "scripts" / "check-windows-imports.py"))["verify"](executable)
        if signing:
            subprocess.run([signing["cmd"], *[arg.replace("%1", str(executable))
                                             for arg in signing["args"]]], check=True)
        with tempfile.TemporaryDirectory(prefix=f"stage-{arch}-", dir=output) as temporary:
            stage = Path(temporary)
            shutil.copy2(executable, stage / executable.name)
            licenses = stage / "licenses"
            bundle["copy_licenses"](licenses)
            bundle["copy_font_notices"](licenses, inputs / "craft-fonts")
            bundle["copy_document_font_notice"](licenses, inputs / "weave-document-fonts")
            bundle["copy_i18n_notices"](licenses, inputs / "craft-i18n")
            installer = output / f"WeaveOffice-{version}-windows-{arch}-setup.exe"
            package_command = [
                "makensis", f"-DVERSION={version}", f"-DARCH={arch}", f"-DSTAGE={stage}",
                f"-DOUTPUT={installer}", f"-DICON={ROOT / 'assets/app-icon/weave-office.ico'}",
            ]
            if signing:
                package_command.append("-DSIGN_COMMAND=" + shlex.join([signing["cmd"], *signing["args"]]))
            subprocess.run([*package_command, str(ROOT / "packaging/windows/installer.nsi")], check=True)
            print(installer, flush=True)


if __name__ == "__main__":
    main()
