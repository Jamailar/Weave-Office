use std::{
    collections::BTreeMap,
    env,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

fn main() {
    if let Err(error) = generate() {
        println!("cargo::error=shared font assets: {error}");
    }
}

fn input(name: &str) -> Result<PathBuf, String> {
    println!("cargo::rerun-if-env-changed={name}");
    env::var_os(name)
        .map(PathBuf::from)
        .ok_or_else(|| format!("{name} is required; use scripts/cargo.py"))?
        .canonicalize()
        .map_err(|error| format!("{name}: {error}"))
}

fn checked_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = root
        .join(relative)
        .canonicalize()
        .map_err(|error| format!("{relative}: {error}"))?;
    if !path.starts_with(root) || !path.is_file() {
        return Err(format!("font input outside its root: {relative}"));
    }
    println!("cargo::rerun-if-changed={}", path.display());
    Ok(path)
}

fn generate() -> Result<(), String> {
    let craft = input("CRAFT_FONTS_DIR")?;
    let document = input("WEAVE_DOCUMENT_FONTS_DIR")?;
    let mut entries = BTreeMap::new();
    for manifest in [
        "fonts/manifest.txt",
        "fonts/simplified-chinese-manifest.txt",
        "fonts/latin-manifest.txt",
    ] {
        let path = checked_path(&craft, manifest)?;
        let text = fs::read_to_string(&path).map_err(|error| error.to_string())?;
        for line in text.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<_> = line.split(" | ").map(str::trim).collect();
            let [_, _, file, _, "OFL-1.1", _, _, _] = fields.as_slice() else {
                return Err(format!("invalid approved font manifest: {manifest}"));
            };
            entries.insert(format!("craft/{file}"), checked_path(&craft, file)?);
        }
    }
    entries.insert(
        "document/NotoSerifCJKsc-Regular.otf".to_owned(),
        checked_path(&document, "NotoSerifCJKsc-Regular.otf")?,
    );

    // Existing, attributed Latin assets also occur in more than one editor.
    let repository =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").ok_or("missing manifest dir")?)
            .join("../..")
            .canonicalize()
            .map_err(|error| error.to_string())?;
    for vendor in ["wordcraft", "pdfcraft"] {
        let directory = repository.join(format!("vendor/{vendor}/assets/fonts"));
        println!("cargo::rerun-if-changed={}", directory.display());
        for entry in fs::read_dir(&directory).map_err(|error| error.to_string())? {
            let path = entry.map_err(|error| error.to_string())?.path();
            if !matches!(
                path.extension().and_then(|s| s.to_str()),
                Some("ttf" | "otf")
            ) {
                continue;
            }
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or("invalid font name")?;
            if let Some(existing) = entries.get(name) {
                if fs::read(existing).map_err(|error| error.to_string())?
                    != fs::read(&path).map_err(|error| error.to_string())?
                {
                    return Err(format!("conflicting bundled font versions: {name}"));
                }
            } else {
                entries.insert(name.to_owned(), path.clone());
            }
            entries.insert(
                format!("{vendor}/{name}"),
                checked_path(&repository, &format!("vendor/{vendor}/assets/fonts/{name}"))?,
            );
        }
    }

    // Compare actual bytes, so aliases are deduplicated without hash collisions
    // or relying on the linker to merge separate include_bytes! allocations.
    let mut storage: BTreeMap<Vec<u8>, String> = BTreeMap::new();
    let mut source = String::new();
    let mut rules = String::from("#[macro_export]\nmacro_rules! font {\n");
    for (key, path) in entries {
        let bytes = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        if bytes.is_empty() {
            return Err(format!("empty font: {}", path.display()));
        }
        let next_name = format!("FONT_{}", storage.len());
        let symbol = storage.entry(bytes).or_insert_with_key(|bytes| {
            let _ = writeln!(
                source,
                "#[doc(hidden)] pub static {next_name}: [u8; {}] = *include_bytes!({:?});",
                bytes.len(),
                path.display().to_string()
            );
            next_name
        });
        let _ = writeln!(rules, "    ({key:?}) => {{ &$crate::{symbol} }};");
    }
    rules.push_str("}\n");
    source.push_str(&rules);
    let output =
        PathBuf::from(env::var_os("OUT_DIR").ok_or("missing output dir")?).join("assets.rs");
    fs::write(output, source).map_err(|error| error.to_string())
}
