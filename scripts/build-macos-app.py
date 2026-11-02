#!/usr/bin/env python3
"""Build a relocatable, local macOS application bundle for Weave Office."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile

from external_cargo import ROOT, build_environment, cargo_metadata, font_policy, i18n_revision
from generate_macos_icons import ICON_FILES, generate as generate_icons


APP_NAME = "Weave Office.app"
BUNDLE_ID = "local.weaveoffice.desktop"
PACKAGE = "weave-office"
BINARY = "weave-office"

DOCUMENT_TYPES = (
    ("Word document", "org.openxmlformats.wordprocessingml.document", "Editor", "word"),
    ("Excel workbook", "org.openxmlformats.spreadsheetml.sheet", "Editor", "sheet"),
    ("PowerPoint presentation", "org.openxmlformats.presentationml.presentation", "Editor", "slides"),
    ("PDF document", "com.adobe.pdf", "Editor", "pdf"),
)


def package_version(metadata: dict) -> str:
    manifest = str(ROOT / "Cargo.toml")
    for package in metadata["packages"]:
        if package["name"] == PACKAGE and package["manifest_path"] == manifest:
            if not any(
                target["name"] == BINARY and "bin" in target["kind"]
                for target in package["targets"]
            ):
                raise ValueError(f"root package has no {BINARY!r} binary target")
            return package["version"]
    raise ValueError(f"root package {PACKAGE!r} not found in cargo metadata")


def copy_licenses(destination: Path) -> None:
    destination.mkdir(parents=True)
    shutil.copy2(ROOT / "vendor" / "README.md", destination / "VENDOR-VERSIONS.md")
    for vendor in ("wordcraft", "gridcraft", "deckcraft", "pdfcraft"):
        source_root = ROOT / "vendor" / vendor
        if not source_root.is_dir():
            raise FileNotFoundError(f"missing upstream source: {source_root}")
        license_files = sorted(
            path for path in source_root.rglob("*")
            if path.is_file()
            and (path.name.startswith(("LICENSE", "COPYING", "OFL")) or path.name in ("NOTICE", "ATTRIBUTION.md"))
            and "tests" not in path.relative_to(source_root).parts
            and path.relative_to(source_root).parts[:2] != ("docs", "brand")
        )
        if not license_files:
            raise FileNotFoundError(f"no license files in {source_root}")
        for source in license_files:
            target = destination / vendor / source.relative_to(source_root)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, target)


def copy_font_notices(destination: Path, fonts_dir: Path) -> None:
    revision, manifests = font_policy()
    font_licenses = destination / "craft-fonts"
    font_licenses.mkdir(parents=True)
    shutil.copy2(fonts_dir / "ATTRIBUTION.md", font_licenses / "ATTRIBUTION.md")
    license_paths = set()
    for manifest in manifests:
        for line in (fonts_dir / manifest).read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if line and not line.startswith("#"):
                license_paths.add(line.split(" | ")[5].strip())
    for relative in sorted(license_paths):
        source = (fonts_dir / relative).resolve(strict=True)
        if not source.is_relative_to(fonts_dir) or not source.is_file():
            raise ValueError(f"invalid craft-fonts licence path: {relative}")
        target = font_licenses / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)
    (font_licenses / "NOTICE").write_text(
        "Builds made with CRAFT_FONTS_DIR embed fonts listed in craft-fonts' ATTRIBUTION.md.\n"
        "Source: https://github.com/storytold/craft-fonts\n"
        f"Commit: {revision}\n",
        encoding="utf-8",
    )


def copy_document_font_notice(destination: Path, fonts_dir: Path) -> None:
    notices = destination / "weave-document-fonts"
    notices.mkdir(parents=True)
    shutil.copy2(fonts_dir / "OFL.txt", notices / "OFL.txt")
    shutil.copy2(fonts_dir / "manifest.txt", notices / "manifest.txt")
    (notices / "NOTICE").write_text(
        "This application embeds Noto Serif CJK SC Regular (OFL-1.1).\n"
        "Font copyright: © 2017-2024 Adobe (http://www.adobe.com/).\n"
        "Source: https://github.com/notofonts/noto-cjk\n"
        "Commit: f8d157532fbfaeda587e826d4cd5b21a49186f7c\n",
        encoding="utf-8",
    )


def copy_i18n_notices(destination: Path, i18n_dir: Path) -> None:
    revision = i18n_revision()
    notices = destination / "craft-i18n"
    notices.mkdir(parents=True)
    for name in ("LICENSE-MIT", "LICENSE-APACHE", "ATTRIBUTION.md"):
        shutil.copy2(i18n_dir / name, notices / name)
    (notices / "NOTICE").write_text(
        "Builds made with CRAFT_I18N_DIR embed translation catalogs from craft-i18n.\n"
        "Source: https://github.com/storytold/craft-i18n\n"
        f"Commit: {revision}\n",
        encoding="utf-8",
    )


def write_info_plist(destination: Path, version: str) -> None:
    info = {
        "CFBundleDevelopmentRegion": "en",
        "CFBundleLocalizations": ["en", "zh-Hans"],
        "CFBundleDisplayName": "Weave Office",
        "CFBundleExecutable": BINARY,
        "CFBundleIconFile": ICON_FILES["app"],
        "CFBundleIdentifier": BUNDLE_ID,
        "CFBundleInfoDictionaryVersion": "6.0",
        "CFBundleName": "Weave Office",
        "CFBundlePackageType": "APPL",
        "CFBundleShortVersionString": version,
        "CFBundleVersion": version,
        "LSMinimumSystemVersion": "11.0",
        "NSHighResolutionCapable": True,
        "CFBundleDocumentTypes": [
            {
                "CFBundleTypeName": name,
                "CFBundleTypeRole": role,
                "CFBundleTypeIconFile": ICON_FILES[kind],
                "LSHandlerRank": "Alternate",
                "LSItemContentTypes": [uti],
            }
            for name, uti, role, kind in DOCUMENT_TYPES
        ],
    }
    with destination.open("wb") as file:
        plistlib.dump(info, file, sort_keys=False)


def validate_output(output: Path, replace: bool) -> None:
    if output.is_symlink():
        raise ValueError(f"destination is a symlink: {output}")
    if not output.exists():
        return
    if not replace:
        raise ValueError(f"destination already exists: {output} (use --replace to replace this app)")
    contents = output / "Contents"
    info_path = contents / "Info.plist"
    if not output.is_dir() or contents.is_symlink() or not contents.is_dir():
        raise ValueError(f"destination is not a valid app bundle: {output}")
    if info_path.is_symlink() or not info_path.is_file():
        raise ValueError(f"destination has no regular Info.plist: {output}")
    try:
        with info_path.open("rb") as file:
            bundle_id = plistlib.load(file).get("CFBundleIdentifier")
    except (AttributeError, OSError, ValueError, TypeError, plistlib.InvalidFileException) as error:
        raise ValueError(f"cannot read destination Info.plist: {info_path}: {error}") from error
    if bundle_id != BUNDLE_ID:
        raise ValueError(f"destination bundle ID is {bundle_id!r}, expected {BUNDLE_ID!r}: {output}")


def commit_bundle(bundle: Path, output: Path, replace: bool) -> None:
    # Check again after the build: the destination may have changed while Cargo ran.
    validate_output(output, replace)
    if not output.exists():
        bundle.rename(output)
        return

    backup_dir = Path(tempfile.mkdtemp(prefix=".weave-office-backup-", dir=output.parent))
    backup = backup_dir / output.name
    try:
        output.rename(backup)
        try:
            bundle.rename(output)
        except OSError as error:
            try:
                backup.rename(output)
            except OSError as rollback_error:
                raise RuntimeError(
                    f"failed to install new app ({error}); old app preserved at {backup}; "
                    f"rollback failed: {rollback_error}"
                ) from rollback_error
            raise
    finally:
        # Never remove the only remaining copy of the old app after a rollback failure.
        if output.exists() and backup.exists():
            shutil.rmtree(backup_dir)
        elif not backup.exists():
            backup_dir.rmdir()


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Build a local Weave Office.app bundle (offline; no install or default-app changes)."
    )
    parser.add_argument(
        "--debug", action="store_true", help="build the dev profile instead of release"
    )
    parser.add_argument(
        "--output", type=Path,
        metavar="APP_PATH", help="destination .app path (default: Cargo target/bundle)"
    )
    parser.add_argument(
        "--replace", action="store_true", help="replace an existing Weave Office app bundle"
    )
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("macOS is required to build a macOS app bundle")
    if shutil.which("cargo") is None:
        parser.error("cargo is required")

    try:
        metadata = cargo_metadata()
        env = build_environment()
        version = package_version(metadata)
        output = (
            args.output.expanduser() if args.output is not None
            else Path(metadata["target_directory"]) / "bundle" / APP_NAME
        )
        output = Path(os.path.abspath(output))
        if output.suffix != ".app":
            parser.error("--output must end in .app")
        validate_output(output, args.replace)
        build_command = [
            "cargo", "build", "--package", PACKAGE, "--bin", BINARY, "--locked", "--offline"
        ]
        if not args.debug:
            build_command.append("--release")
        subprocess.run(build_command, cwd=ROOT, env=env, check=True)

        profile = "debug" if args.debug else "release"
        executable = Path(metadata["target_directory"]) / profile / BINARY
        if not executable.is_file() or not os.access(executable, os.X_OK):
            raise FileNotFoundError(f"built executable not found or not executable: {executable}")

        output.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix=".weave-office-", dir=output.parent) as temporary:
            bundle = Path(temporary) / APP_NAME
            contents = bundle / "Contents"
            macos = contents / "MacOS"
            macos.mkdir(parents=True)
            resources = contents / "Resources"
            resources.mkdir()
            generate_icons(resources)
            shutil.copy2(executable, macos / BINARY, follow_symlinks=True)
            write_info_plist(contents / "Info.plist", version)
            copy_licenses(resources / "licenses")
            copy_font_notices(resources / "licenses", Path(env["CRAFT_FONTS_DIR"]))
            copy_document_font_notice(resources / "licenses", Path(env["WEAVE_DOCUMENT_FONTS_DIR"]))
            copy_i18n_notices(resources / "licenses", Path(env["CRAFT_I18N_DIR"]))
            subprocess.run(
                ["codesign", "--sign", "-", "--force", "--timestamp=none", str(bundle)],
                check=True,
            )
            subprocess.run(["codesign", "--verify", "--strict", str(bundle)], check=True)
            commit_bundle(bundle, output, args.replace)
    except (OSError, RuntimeError, ValueError, subprocess.CalledProcessError, KeyError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1

    print(output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
