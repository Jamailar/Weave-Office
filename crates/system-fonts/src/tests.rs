use super::*;
use skrifa::raw::{FileRef, TableProvider};
use skrifa::{FontRef, MetadataProvider, string::StringId};

/// Every installed font plus the build's font inputs, when present: the corpus the catalog is
/// checked against.
fn corpus() -> Vec<PathBuf> {
    let mut dirs = font_dirs();
    for var in ["CRAFT_FONTS_DIR", "WEAVE_DOCUMENT_FONTS_DIR"] {
        if let Some(d) = std::env::var_os(var) {
            dirs.push(d.into());
        }
    }
    dirs
}

fn names(f: &FontRef<'_>) -> Vec<(u16, Option<String>, String)> {
    let mut ids = vec![1u16, 2, 4, 6, 16, 17];
    ids.extend(f.named_instances().iter().map(|ni| ni.subfamily_name_id().to_u16()));
    let mut out: Vec<_> = ids
        .into_iter()
        .flat_map(|id| f.localized_strings(StringId::new(id)).map(move |s| (id, s.language().map(str::to_owned), s.to_string())))
        .collect();
    out.sort();
    out
}

#[test]
fn the_catalog_reads_the_names_axes_and_code_pages_the_whole_file_has() {
    let catalog = Catalog { dirs: vec![], faces: scan_dirs(&corpus()), generation: 0 };
    let mut checked = 0;
    for (path, faces) in catalog.files() {
        let data = std::fs::read(path).unwrap();
        for face in faces {
            let full = FontRef::from_index(&data, face.index).unwrap();
            let mini = FontRef::new(&face.tables).unwrap();
            assert_eq!(names(&mini), names(&full), "{} #{}", path.display(), face.index);
            assert_eq!(mini.axes().len(), full.axes().len(), "{}", path.display());
            assert_eq!(mini.named_instances().len(), full.named_instances().len(), "{}", path.display());
            assert_eq!(
                mini.os2().ok().and_then(|t| t.ul_code_page_range_1()),
                full.os2().ok().and_then(|t| t.ul_code_page_range_1()),
                "{}",
                path.display()
            );
            assert_eq!(face.has_outlines(), OUTLINE_TABLES.iter().any(|t| full.table_data(skrifa::Tag::new(t)).is_some()));
            checked += 1;
        }
    }
    eprintln!("checked {checked} faces");
}

#[test]
fn every_face_of_a_collection_is_cataloged() {
    let catalog = Catalog { dirs: vec![], faces: scan_dirs(&corpus()), generation: 0 };
    for (path, faces) in catalog.files() {
        let data = std::fs::read(path).unwrap();
        let count = match FileRef::new(&data).unwrap() {
            FileRef::Font(_) => 1,
            FileRef::Collection(c) => c.len(),
        };
        assert_eq!(faces.len() as u32, count, "{}", path.display());
    }
}

#[test]
fn the_kept_tables_are_a_small_part_of_the_files() {
    let faces = scan_dirs(&corpus());
    let kept: usize = faces.iter().map(|f| f.tables.len()).sum();
    let files: u64 = Catalog { dirs: vec![], faces: faces.clone(), generation: 0 }.files().map(|(_, f)| f[0].file_len).sum();
    eprintln!("{} faces, {kept} bytes kept of {files}", faces.len());
    assert!(faces.is_empty() || (kept as u64) * 20 < files, "{kept} of {files}");
}

#[test]
fn damaged_and_misnamed_files_yield_nothing() {
    let dir = std::env::temp_dir().join(format!("weave-system-fonts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("short.ttf"), b"0123").unwrap();
    std::fs::write(dir.join("junk.ttf"), vec![0xffu8; 4096]).unwrap();
    let mut ttc = b"ttcf\0\x01\0\0\xff\xff\xff\xff".to_vec();
    ttc.extend_from_slice(&[0xff; 64]);
    std::fs::write(dir.join("huge-count.ttc"), ttc).unwrap();
    std::fs::write(dir.join("notes.txt"), b"not a font").unwrap();
    assert!(scan_dirs(std::slice::from_ref(&dir)).is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_folder_linked_back_into_itself_is_read_once() {
    let Some(src) = scan_dirs(&corpus()).into_iter().next() else { return };
    let dir = std::env::temp_dir().join(format!("weave-system-fonts-loop-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ext = src.path.extension().unwrap().to_owned();
    std::fs::copy(&src.path, dir.join("a").with_extension(ext)).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&dir, dir.join("loop")).unwrap();
    let faces = scan_dirs(std::slice::from_ref(&dir));
    let files = Catalog { dirs: vec![], faces, generation: 0 }.files().count();
    assert_eq!(files, 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_rescan_is_a_later_generation_and_catalog_reuses_it() {
    let first = catalog();
    assert!(Arc::ptr_eq(&first, &catalog()));
    let second = rescan();
    assert!(second.generation > first.generation);
    assert!(Arc::ptr_eq(&second, &catalog()));
    assert_eq!(first.faces.len(), second.faces.len());
}

#[test]
fn fontconfig_folders_resolve_like_fontconfig() {
    let text = r#"<dir>/opt/fonts</dir><dir prefix="xdg">fonts</dir><dir>~/my-fonts</dir><dirname>x</dirname><dir>relative</dir>"#;
    let dirs = fontconfig_dirs(text, None, Some(Path::new("/home/u")));
    assert_eq!(dirs, [PathBuf::from("/opt/fonts"), "/home/u/.local/share/fonts".into(), "/home/u/my-fonts".into()]);
}
