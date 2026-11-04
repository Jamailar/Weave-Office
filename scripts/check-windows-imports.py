#!/usr/bin/env python3
"""Check that a Windows executable only imports DLLs every supported Windows has.

A DLL in the import table is loaded before `main` runs. If one is missing, Windows refuses to start
the program with "The code execution cannot proceed because X.dll was not found", and the app has
no chance to show anything. Optional system components (for example `windows.data.pdf.dll`, absent
from some N/KN, LTSC, Server and trimmed-down installations) must therefore be loaded at run time
with `LoadLibraryExW` and degrade gracefully, never imported directly.

Usage: python3 scripts/check-windows-imports.py weave-office.exe [...]
"""

from __future__ import annotations

import struct
import sys
from pathlib import Path

# DLLs present on every Windows 10/11 edition the app supports, including N/KN, LTSC and Server
# with Desktop Experience. Compare in lower case. Add a DLL here only after checking that it ships
# with all of them; anything optional belongs behind LoadLibraryExW instead.
CORE_DLLS = frozenset({
    "advapi32.dll",
    "bcrypt.dll",
    "bcryptprimitives.dll",
    "cfgmgr32.dll",
    "combase.dll",
    "comctl32.dll",
    "comdlg32.dll",
    "crypt32.dll",
    "dbghelp.dll",
    "d2d1.dll",
    "d3d11.dll",
    "d3d12.dll",
    "dwmapi.dll",
    "dwrite.dll",
    "dxgi.dll",
    "gdi32.dll",
    "imm32.dll",
    "hid.dll",
    "iphlpapi.dll",
    "kernel32.dll",
    "ktmw32.dll",
    "msvcrt.dll",
    "ncrypt.dll",
    "ntdll.dll",
    "ole32.dll",
    "oleaut32.dll",
    "opengl32.dll",
    "powrprof.dll",
    "propsys.dll",
    "rpcrt4.dll",
    "secur32.dll",
    "setupapi.dll",
    "shell32.dll",
    "shlwapi.dll",
    "uiautomationcore.dll",
    "user32.dll",
    "userenv.dll",
    "uxtheme.dll",
    "version.dll",
    "windowscodecs.dll",
    "winmm.dll",
    "winspool.drv",
    "ws2_32.dll",
})

# API sets resolve inside the loader on Windows 10 and later; these families are part of OneCore.
CORE_API_SET_PREFIXES = ("api-ms-win-core-", "api-ms-win-crt-", "api-ms-win-security-", "api-ms-win-shcore-")


def _rva_reader(data: bytes):
    if data[:2] != b"MZ":
        raise ValueError("not a Windows executable")
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe:pe + 4] != b"PE\0\0":
        raise ValueError("not a PE file")
    sections, optional_size = struct.unpack_from("<H12xH", data, pe + 6)
    optional = pe + 24
    magic = struct.unpack_from("<H", data, optional)[0]
    directories = optional + {0x10B: 96, 0x20B: 112}[magic]
    table = optional + optional_size
    spans = []
    for i in range(sections):
        size, address, raw_size, raw = struct.unpack_from("<4xIII I", data, table + 40 * i + 4)
        spans.append((address, max(size, raw_size), raw))

    def offset(rva: int) -> int:
        for address, size, raw in spans:
            if address <= rva < address + size:
                return rva - address + raw
        raise ValueError(f"RVA {rva:#x} is outside every section")

    def directory(index: int) -> tuple[int, int]:
        return struct.unpack_from("<II", data, directories + 8 * index)

    return offset, directory


def _name(data: bytes, at: int) -> str:
    return data[at:data.index(b"\0", at)].decode("ascii")


def imports(path: Path) -> tuple[list[str], list[str]]:
    """The DLLs `path` imports at load time, and those it delay-loads."""
    data = path.read_bytes()
    offset, directory = _rva_reader(data)
    static = []
    rva, _ = directory(1)
    if rva:
        at = offset(rva)
        while True:
            lookup, _, _, name, thunks = struct.unpack_from("<IIIII", data, at)
            if not (lookup or name or thunks):
                break
            static.append(_name(data, offset(name)))
            at += 20
    delayed = []
    rva, _ = directory(13)
    if rva:
        at = offset(rva)
        while True:
            fields = struct.unpack_from("<8I", data, at)
            if not any(fields):
                break
            delayed.append(_name(data, offset(fields[1])))
            at += 32
    return static, delayed


def optional_imports(path: Path) -> list[str]:
    """Load-time imports that some supported Windows installation may not have."""
    static, _ = imports(path)
    return sorted({
        dll for dll in static
        if dll.lower() not in CORE_DLLS and not dll.lower().startswith(CORE_API_SET_PREFIXES)
    }, key=str.lower)


def verify(path: Path) -> None:
    unexpected = optional_imports(path)
    if unexpected:
        raise SystemExit(
            f"{path} imports DLLs that some Windows installations lack, so it would not start there: "
            f"{', '.join(unexpected)}. Load them at run time (LoadLibraryExW + GetProcAddress) and "
            f"degrade, or add them to CORE_DLLS in scripts/check-windows-imports.py once checked "
            f"to ship with every supported edition."
        )


def main() -> None:
    if len(sys.argv) < 2:
        raise SystemExit(__doc__)
    for argument in sys.argv[1:]:
        path = Path(argument)
        static, delayed = imports(path)
        print(f"{path}: imports {', '.join(sorted({d.lower() for d in static}))}")
        if delayed:
            print(f"{path}: delay-loads {', '.join(sorted({d.lower() for d in delayed}))}")
        verify(path)
    print("OK: every load-time import ships with all supported Windows editions")


if __name__ == "__main__":
    main()
