"""Validate the machine-local external Cargo target before running Cargo."""

from __future__ import annotations

import json
import os
from pathlib import Path
import runpy
import subprocess
import sys

from weave_volume import dev_volume


ROOT = Path(__file__).resolve().parent.parent
VOLUME = dev_volume()
FONTS_DIR = VOLUME / "DevCache/WeaveOffice/inputs/craft-fonts"
DOCUMENT_FONTS_DIR = VOLUME / "DevCache/WeaveOffice/inputs/weave-document-fonts"
I18N_DIR = VOLUME / "DevCache/WeaveOffice/inputs/craft-i18n"


def font_policy() -> tuple[str, tuple[str, ...]]:
    """Use the preparation script's pinned revision and manifest list as the source of truth."""
    policy = runpy.run_path(str(ROOT / "scripts" / "prepare-fonts.py"))
    if policy["DEFAULT_DIR"] != FONTS_DIR:
        raise RuntimeError("craft-fonts input paths differ between build and preparation scripts")
    return policy["REV"], policy["MANIFESTS"]


def i18n_revision() -> str:
    """Use WordCraft's pinned translation revision through its preparation script."""
    policy = runpy.run_path(str(ROOT / "scripts" / "prepare-i18n.py"))
    if policy["DEFAULT_DIR"] != I18N_DIR:
        raise RuntimeError("craft-i18n input paths differ between build and preparation scripts")
    return policy["pinned_revision"]()


def require_volume() -> None:
    if not VOLUME.is_mount():
        raise RuntimeError(
            f"{VOLUME} is not mounted; mount the external volume (WEAVE_DEV_VOLUME) before building or testing"
        )


def cargo_metadata() -> dict:
    require_volume()
    result = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked", "--offline"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    metadata = json.loads(result.stdout)
    target = Path(metadata["target_directory"]).resolve(strict=False)
    volume = VOLUME.resolve(strict=True)
    if target == volume or not target.is_relative_to(volume):
        raise RuntimeError(
            f"Cargo target directory is outside the mounted external volume: {target}"
        )
    return metadata


def build_environment() -> dict[str, str]:
    """Require the pinned font and translation inputs for every native Cargo build."""
    require_volume()
    prepare = ROOT / "scripts" / "prepare-fonts.py"
    hint = "python3 scripts/prepare-fonts.py"
    if not FONTS_DIR.is_dir():
        raise RuntimeError(f"required craft-fonts input is missing at {FONTS_DIR}; run {hint}")
    font_policy()
    font_path = FONTS_DIR.resolve(strict=True)
    if not font_path.is_relative_to(VOLUME.resolve(strict=True)):
        raise RuntimeError(f"craft-fonts input resolves outside the external volume: {font_path}; run {hint}")
    result = subprocess.run(
        [sys.executable, str(prepare), "--verify-only", "--dir", str(font_path)],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if result.returncode:
        detail = result.stderr.strip() or result.stdout.strip()
        raise RuntimeError(f"required craft-fonts input is invalid: {detail}; run {hint}")

    document_prepare = ROOT / "scripts" / "prepare-document-fonts.py"
    document_hint = "python3 scripts/prepare-document-fonts.py"
    if not DOCUMENT_FONTS_DIR.is_dir():
        raise RuntimeError(f"required document fonts input is missing at {DOCUMENT_FONTS_DIR}; run {document_hint}")
    document_path = DOCUMENT_FONTS_DIR.resolve(strict=True)
    if not document_path.is_relative_to(VOLUME.resolve(strict=True)):
        raise RuntimeError(f"document fonts input resolves outside the external volume: {document_path}; run {document_hint}")
    document_result = subprocess.run(
        [sys.executable, str(document_prepare), "--verify-only", "--dir", str(document_path)],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if document_result.returncode:
        detail = document_result.stderr.strip() or document_result.stdout.strip()
        raise RuntimeError(f"required document fonts input is invalid: {detail}; run {document_hint}")

    i18n_prepare = ROOT / "scripts" / "prepare-i18n.py"
    i18n_hint = "python3 scripts/prepare-i18n.py"
    if not I18N_DIR.is_dir():
        raise RuntimeError(f"required craft-i18n input is missing at {I18N_DIR}; run {i18n_hint}")
    i18n_revision()
    i18n_path = I18N_DIR.resolve(strict=True)
    if not i18n_path.is_relative_to(VOLUME.resolve(strict=True)):
        raise RuntimeError(f"craft-i18n input resolves outside the external volume: {i18n_path}; run {i18n_hint}")
    i18n_result = subprocess.run(
        [sys.executable, str(i18n_prepare), "--verify-only", "--dir", str(i18n_path)],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if i18n_result.returncode:
        detail = i18n_result.stderr.strip() or i18n_result.stdout.strip()
        raise RuntimeError(f"required craft-i18n input is invalid: {detail}; run {i18n_hint}")

    env = os.environ.copy()
    env["CRAFT_FONTS_DIR"] = str(font_path)
    env["CRAFT_FONTS_REQUIRED"] = "1"
    env["WEAVE_DOCUMENT_FONTS_DIR"] = str(document_path)
    env["WEAVE_DOCUMENT_FONTS_REQUIRED"] = "1"
    env["CRAFT_I18N_DIR"] = str(i18n_path)
    env["CRAFT_I18N_REQUIRED"] = "1"
    return env
