//! Windows only: embed the app icon and version info (VERSIONINFO) into `weave-office.exe`, so it
//! shows in Explorer, the Start menu, the taskbar, Alt-Tab and the installer's shortcuts.
//!
//! On every other target this does nothing. A missing resource compiler is a warning, so a
//! cross-compile still links, unless `WEAVE_REQUIRE_WINRES=1` turns it into an error (release
//! builds set it: see `scripts/build-windows-app.py`).

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/app-icon/weave-office.ico");
    println!("cargo:rerun-if-env-changed=WEAVE_REQUIRE_WINRES");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/app-icon/weave-office.ico")
        .set("ProductName", "Weave Office")
        .set("FileDescription", "Weave Office")
        .set("LegalCopyright", "Copyright (c) Weave Office contributors. MIT License.")
        .set("OriginalFilename", "weave-office.exe")
        .set("InternalName", "weave-office");
    if let Err(e) = res.compile() {
        if std::env::var_os("WEAVE_REQUIRE_WINRES").is_some() {
            eprintln!("embedding Windows resources failed: {e}");
            std::process::exit(1);
        }
        println!("cargo:warning=weave-office.exe built without icon/version resources: {e}");
    }
}
