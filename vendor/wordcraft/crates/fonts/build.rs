//! Optional craft-fonts build input (https://github.com/storytold/craft-fonts, recipe from its
//! `docs/integration.md`). With `CRAFT_FONTS_DIR=<checkout>` the fonts in its
//! `fonts/manifest.txt` are embedded as `CRAFT_FONTS`; unset, `CRAFT_FONTS` is empty. Web
//! (wasm32) builds embed only the Georgian interface face (about 230 KB), to stay within the web
//! size budget; the Japanese one is 4.7 MB. With `CRAFT_FONTS_REQUIRED` (releases), a native
//! build also fails when the checkout has no font for a CJK interface language (Japanese,
//! Simplified Chinese), as an outdated pin once did (#241). It only reads the local checkout: no
//! network.
use std::fmt::Write as _;
use std::path::PathBuf;

fn main() {
    println!("cargo::rerun-if-env-changed=CRAFT_FONTS_DIR");
    println!("cargo::rerun-if-env-changed=CRAFT_FONTS_REQUIRED");
    println!("cargo::rerun-if-env-changed=WEAVE_DOCUMENT_FONTS_DIR");
    println!("cargo::rerun-if-env-changed=WEAVE_DOCUMENT_FONTS_REQUIRED");
    let mut src = String::from("pub static CRAFT_FONTS: &[CraftFont] = &[\n");
    if let Some(dir) = std::env::var_os("CRAFT_FONTS_DIR").map(PathBuf::from) {
        match craft_fonts(&dir) {
            Ok((entries, covered)) => {
                src.push_str(&entries);
                // Each CJK interface language needs its own face: without the Chinese one, Chinese
                // menus fall back to the Japanese face and show boxes for simplified hanzi (#241).
                let wasm = std::env::var("CARGO_CFG_TARGET_ARCH").is_ok_and(|a| a == "wasm32");
                if std::env::var_os("CRAFT_FONTS_REQUIRED").is_some() && !wasm {
                    for s in REQUIRED_SCRIPTS.iter().filter(|s| !covered.iter().any(|c| c == *s)) {
                        println!("cargo::error=CRAFT_FONTS_DIR={}: no {s} font in fonts/manifest.txt (craft-fonts too old?)", dir.display());
                    }
                }
            }
            Err(e) if std::env::var_os("CRAFT_FONTS_REQUIRED").is_some() => {
                println!("cargo::error=CRAFT_FONTS_DIR={}: {e}", dir.display());
            }
            Err(e) => println!("cargo::warning=building without craft-fonts: CRAFT_FONTS_DIR={}: {e}", dir.display()),
        }
    }
    // Append after craft-fonts so the first Hans face remains the UI's Noto Sans CJK SC.
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("wasm32") {
        if let Some(dir) = std::env::var_os("WEAVE_DOCUMENT_FONTS_DIR").map(PathBuf::from) {
            match document_serif(&dir) {
                Ok(entry) => src.push_str(&entry),
                Err(e) if std::env::var_os("WEAVE_DOCUMENT_FONTS_REQUIRED").is_some() => {
                    println!("cargo::error=WEAVE_DOCUMENT_FONTS_DIR={}: {e}", dir.display());
                }
                Err(e) => println!("cargo::warning=building without document serif: WEAVE_DOCUMENT_FONTS_DIR={}: {e}", dir.display()),
            }
        } else if std::env::var_os("WEAVE_DOCUMENT_FONTS_REQUIRED").is_some() {
            println!("cargo::error=WEAVE_DOCUMENT_FONTS_REQUIRED needs WEAVE_DOCUMENT_FONTS_DIR");
        }
    }
    src.push_str("];\n");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("craft_fonts.rs");
    if let Err(e) = std::fs::write(&out, src) {
        println!("cargo::error=writing {}: {e}", out.display());
    }
}

fn document_serif(dir: &std::path::Path) -> Result<String, String> {
    let dir = dir.canonicalize().map_err(|e| format!("input directory: {e}"))?;
    let manifest = dir.join("manifest.txt");
    println!("cargo::rerun-if-changed={}", manifest.display());
    let text = std::fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let lines: Vec<_> = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
    let [line] = lines.as_slice() else { return Err("expected one document serif manifest entry".into()) };
    let fields: Vec<_> = line.split(" | ").map(str::trim).collect();
    let ["Noto Serif CJK SC", "Regular", "NotoSerifCJKsc-Regular.otf", "Hans", "OFL-1.1", "OFL.txt", _, _] = fields.as_slice() else {
        return Err("unexpected document serif manifest entry".into());
    };
    let path = dir.join("NotoSerifCJKsc-Regular.otf").canonicalize().map_err(|e| format!("font file: {e}"))?;
    if path.parent() != Some(dir.as_path()) {
        return Err("document serif font resolves outside input".into());
    }
    println!("cargo::rerun-if-changed={}", path.display());
    let bytes = if std::env::var_os("CARGO_FEATURE_SHARED_FONT_ASSETS").is_some() {
        "weave_font_assets::font!(\"document/NotoSerifCJKsc-Regular.otf\")".to_owned()
    } else {
        format!("include_bytes!({:?})", path.display().to_string())
    };
    Ok(format!("    CraftFont {{ family: \"Noto Serif CJK SC\", style: \"Regular\", scripts: &[\"Hans\"], bytes: {bytes} }},\n"))
}

/// Scripts a release build (`CRAFT_FONTS_REQUIRED`, native) must embed a font for: the CJK
/// interface languages' (Japanese, Simplified Chinese).
const REQUIRED_SCRIPTS: &[&str] = &["Jpan", "Hans"];

/// One `CraftFont { .. }` initialiser per manifest line, and the scripts the embedded fonts cover.
fn craft_fonts(dir: &std::path::Path) -> Result<(String, Vec<String>), String> {
    let manifest = dir.join("fonts/manifest.txt");
    println!("cargo::rerun-if-changed={}", manifest.display());
    let text = std::fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let wasm = std::env::var("CARGO_CFG_TARGET_ARCH").is_ok_and(|a| a == "wasm32");
    let mut out = String::new();
    let mut covered = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let f: Vec<&str> = line.split(" | ").map(str::trim).collect();
        let [family, style, file, scripts, ..] = f.as_slice() else {
            return Err(format!("malformed manifest line: {line}"));
        };
        if wasm && !scripts.split(',').any(|s| s.trim() == "Geor") {
            continue;
        }
        let path = dir.join(file).canonicalize().map_err(|e| format!("{file}: {e}"))?;
        println!("cargo::rerun-if-changed={}", path.display());
        covered.extend(scripts.split(',').map(|s| s.trim().to_string()));
        let scripts: Vec<String> = scripts.split(',').map(|s| format!("{:?}", s.trim())).collect();
        let bytes = if std::env::var_os("CARGO_FEATURE_SHARED_FONT_ASSETS").is_some() {
            format!("weave_font_assets::font!({:?})", format!("craft/{file}"))
        } else {
            format!("include_bytes!({:?})", path.display().to_string())
        };
        let _ = writeln!(out, "    CraftFont {{ family: {family:?}, style: {style:?}, scripts: &[{}], bytes: {bytes} }},", scripts.join(", "),);
    }
    Ok((out, covered))
}
