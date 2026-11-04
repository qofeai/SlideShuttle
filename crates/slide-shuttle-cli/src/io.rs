use slide_shuttle_core::{Deck, Error, FontResource, Kind, Node, Resources, Result};
use std::{
    fs,
    io::Write,
    path::{Component, Path},
    sync::atomic::{AtomicU64, Ordering},
};

fn fail(s: impl Into<String>) -> Error {
    Error::Invalid(s.into())
}
pub fn read(path: &Path, max: u64) -> Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > max {
        return Err(fail(format!("file exceeds size limit: {}", path.display())));
    }
    use std::io::Read;
    let mut b = Vec::new();
    file.take(max + 1).read_to_end(&mut b)?;
    if b.len() as u64 > max {
        return Err(fail("file exceeds size limit"));
    }
    Ok(b)
}
fn text(path: &Path) -> Result<String> {
    String::from_utf8(read(path, 4 * 1024 * 1024)?)
        .map_err(|_| fail(format!("source must be UTF-8: {}", path.display())))
}
/// Read the project (entry, libraries, pages) and exactly the fonts and images it declares.
/// Every referenced path resolves inside the entry file's directory.
pub fn load(
    input: &Path,
    fonts: &[std::path::PathBuf],
    page: Option<&str>,
) -> Result<(Deck, Resources, usize, bool)> {
    if input
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("qppt"))
    {
        let mut project = project(input)?;
        let mut deck = project.deck()?;
        let first = select(&mut deck, page)?;
        let mut resources = project.take_resources(&deck)?;
        for font in fonts {
            resources.fonts.push(FontResource {
                family: None,
                bytes: read(font, 64 * 1024 * 1024)?,
            });
        }
        return Ok((deck, resources, first, project.manifest.imported));
    }
    let entry_path = if input.is_dir() {
        input.join("deck.xml")
    } else {
        input.to_owned()
    };
    let input = entry_path.as_path();
    let root = input
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()?;
    let entry = input
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let source = text(input)?;
    slide_shuttle_core::package::require_project(&source)?;
    let mut deck =
        slide_shuttle_core::parse_project(&entry, &source, |path| text(&local(&root, path)?))?;
    // Select before loading media: inspecting one page must not decode/read unrelated
    // images or fail because another page's asset is still being authored.
    let mut first = 0;
    if let Some(page) = page {
        first = deck
            .slides
            .iter()
            .position(|s| s.id == page)
            .ok_or_else(|| fail(format!("no slide with id {page}")))?;
        deck.slides = vec![deck.slides.swap_remove(first)];
    }
    let mut resources = Resources::default();
    for font in &deck.fonts {
        resources.fonts.push(FontResource {
            family: Some(font.family.clone()),
            bytes: read(&local(&root, &font.src)?, 64 * 1024 * 1024)?,
        });
    }
    for path in fonts {
        resources.fonts.push(FontResource {
            family: None,
            bytes: read(path, 64 * 1024 * 1024)?,
        });
    }
    let mut imported = false;
    let manifest = root.join("manifest.json");
    if manifest.exists() {
        let manifest: slide_shuttle_core::package::Manifest =
            serde_json::from_slice(&read(&manifest, 1024 * 1024)?)?;
        manifest.validate()?;
        imported = manifest.imported;
        for src in manifest.fonts {
            resources.fonts.push(FontResource {
                family: None,
                bytes: read(&local(&root, &src)?, 64 * 1024 * 1024)?,
            });
        }
    }
    if resources.fonts.is_empty() {
        return Err(fail(
            "supply <Font family=... src=.../> or repeat --font FILE; system font discovery is disabled",
        ));
    }
    fn visit(node: &Node, root: &Path, deck: &Deck, resources: &mut Resources) -> Result<()> {
        if let Kind::Image { src, .. } | Kind::Icon { src } = &node.kind {
            let path = deck.assets.get(src).map(String::as_str).unwrap_or(src);
            if !resources.images.contains_key(src) {
                resources
                    .images
                    .insert(src.clone(), read(&local(root, path)?, 32 * 1024 * 1024)?);
            }
        }
        for child in &node.children {
            visit(child, root, deck, resources)?;
        }
        Ok(())
    }
    for slide in &deck.slides {
        for node in &slide.nodes {
            visit(node, &root, &deck, &mut resources)?;
        }
        if let Some(src) = slide_shuttle_core::layout::background_image(&slide.background) {
            let path = deck.assets.get(src).map(String::as_str).unwrap_or(src);
            if !resources.images.contains_key(src) {
                resources
                    .images
                    .insert(src.into(), read(&local(&root, path)?, 32 * 1024 * 1024)?);
            }
        }
    }
    let total: usize = resources.fonts.iter().map(|f| f.bytes.len()).sum::<usize>()
        + resources.images.values().map(Vec::len).sum::<usize>();
    if total > 256 * 1024 * 1024 {
        return Err(fail("resource budget exceeds 256 MiB"));
    }
    Ok((deck, resources, first, imported))
}
fn select(deck: &mut Deck, page: Option<&str>) -> Result<usize> {
    if let Some(page) = page {
        let first = deck
            .slides
            .iter()
            .position(|s| s.id == page)
            .ok_or_else(|| fail(format!("no slide with id {page}")))?;
        deck.slides = vec![deck.slides.swap_remove(first)];
        Ok(first)
    } else {
        Ok(0)
    }
}
pub fn project(input: &Path) -> Result<slide_shuttle_core::package::Project> {
    use slide_shuttle_core::{
        baseline::Baseline,
        package::{MAX_BYTES, Manifest, Project},
    };
    if input
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("qppt"))
    {
        return Project::read(&read(input, MAX_BYTES as u64)?);
    }
    let entry_path = if input.is_dir() {
        input.join("deck.xml")
    } else {
        input.to_owned()
    };
    let root = entry_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()?;
    let entry = entry_path
        .file_name()
        .ok_or_else(|| fail("missing project entry"))?
        .to_string_lossy();
    let manifest_path = root.join("manifest.json");
    let mut manifest: Manifest = if manifest_path.exists() {
        serde_json::from_slice(&read(&manifest_path, 1024 * 1024)?)?
    } else {
        Manifest::default()
    };
    let baseline_path = root.join(".slide-shuttle/base.json");
    if baseline_path.exists() {
        let base: Baseline = serde_json::from_slice(&read(&baseline_path, 64 * 1024 * 1024)?)?;
        if base.imported {
            manifest.imported = true;
            manifest.losses = base.native.losses;
        }
    }
    Project::load(&entry, manifest, |name| {
        read(&local(&root, name)?, 64 * 1024 * 1024)
    })
}
pub fn source_root(input: &Path) -> &Path {
    if input.is_dir() {
        input
    } else {
        input
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
    }
}
fn local(root: &Path, src: &str) -> Result<std::path::PathBuf> {
    let p = Path::new(src);
    if p.is_absolute()
        || p.components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(fail(
            "declared resources must use relative paths within the source directory",
        ));
    }
    let path = root.join(p).canonicalize()?;
    if !path.starts_with(root) {
        return Err(fail("resource symlink escapes source directory"));
    }
    Ok(path)
}
static COUNTER: AtomicU64 = AtomicU64::new(0);
/// New file only. Hard-link publication fails atomically if another process has created the target.
pub fn write_new(path: &Path, data: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(
        ".slide-shuttle-tmp-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        fs::hard_link(&tmp, path)?;
        Ok::<_, std::io::Error>(())
    })();
    let _ = fs::remove_file(&tmp);
    result.map_err(Error::Io)
}

/// Render to a private sibling directory one file at a time, then publish into a newly
/// reserved directory. Render failures leave no output and don't retain the whole deck
/// in memory. Exclusive directory creation preserves concurrently created destinations.
pub fn write_directory(path: &Path, render: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
    if path.exists() {
        return Err(fail(
            "render output directory already exists; choose a new path",
        ));
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let stage = tempfile::Builder::new()
        .prefix(".slide-shuttle-render-")
        .tempdir_in(parent)?;
    render(stage.path())?;
    fs::create_dir(path)?;
    let mut published = Vec::new();
    let result = (|| {
        fn collect(
            root: &Path,
            dir: &Path,
            files: &mut Vec<std::path::PathBuf>,
        ) -> std::io::Result<()> {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    collect(root, &entry.path(), files)?;
                } else {
                    files.push(
                        entry
                            .path()
                            .strip_prefix(root)
                            .expect("staged file stays inside root")
                            .to_owned(),
                    );
                }
            }
            Ok(())
        }
        let mut entries = Vec::new();
        collect(stage.path(), stage.path(), &mut entries)?;
        // The manifest is the completion marker for readers of a newly published deck.
        entries.sort_by_key(|entry| entry == Path::new("manifest.json"));
        for entry in entries {
            let target = path.join(&entry);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::hard_link(stage.path().join(entry), &target)?;
            published.push(target);
        }
        Ok::<_, std::io::Error>(())
    })();
    if result.is_err() {
        for file in published {
            let _ = fs::remove_file(file);
        }
        fn empty_dirs(path: &Path) {
            if let Ok(entries) = fs::read_dir(path) {
                for entry in entries.flatten() {
                    if entry.file_type().is_ok_and(|t| t.is_dir()) {
                        empty_dirs(&entry.path());
                    }
                }
            }
            let _ = fs::remove_dir(path);
        }
        empty_dirs(path);
        let _ = fs::remove_dir(path);
    }
    result.map_err(Error::Io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_render_failure_cleans_up_and_publication_preserves_racing_writer() {
        let root = tempfile::tempdir().unwrap();
        let out = root.path().join("preview");
        let error = write_directory(&out, |stage| {
            write_new(&stage.join("slide-001.png"), b"first page")?;
            Err(fail("later page failed"))
        })
        .unwrap_err();
        assert!(error.to_string().contains("later page failed"));
        assert!(!out.exists());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
        assert!(
            write_directory(&out, |stage| {
                write_new(&stage.join("slide-001.png"), b"our page")?;
                fs::create_dir(&out)?;
                fs::write(out.join("external.txt"), b"other writer")?;
                Ok(())
            })
            .is_err()
        );
        assert_eq!(fs::read(out.join("external.txt")).unwrap(), b"other writer");
        assert!(!out.join("slide-001.png").exists());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
}
