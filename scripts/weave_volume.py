"""Locate the external development volume used for build outputs and large inputs."""

from __future__ import annotations

import os
from pathlib import Path


def dev_volume() -> Path:
    """Return the mount point named by WEAVE_DEV_VOLUME, e.g. /Volumes/MyDrive."""
    value = os.environ.get("WEAVE_DEV_VOLUME")
    if not value:
        raise SystemExit(
            "WEAVE_DEV_VOLUME is not set; export it as the mount point of the external build "
            "volume (for example WEAVE_DEV_VOLUME=/Volumes/MyDrive) before running this script"
        )
    return Path(value).expanduser()
