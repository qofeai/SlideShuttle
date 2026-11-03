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
pub fn load(input: &Path, fonts: &[std::path::PathBuf]) -> Result<(Deck, Resources)> {
    let root = input
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()?;
    let entry = input
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let deck = slide_shuttle_core::parse_project(&entry, &text(input)?, |path| {
        text(&local(&root, path)?)
    })?;
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
    if resources.fonts.is_empty() {
        return Err(fail(
            "supply <Font family=... src=.../> or repeat --font FILE; system font discovery is disabled",
        ));
    }
    fn visit(node: &Node, root: &Path, deck: &Deck, resources: &mut Resources) -> Result<()> {
        if let Kind::Image { src } = &node.kind {
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
    }
    let total: usize = resources.fonts.iter().map(|f| f.bytes.len()).sum::<usize>()
        + resources.images.values().map(Vec::len).sum::<usize>();
    if total > 256 * 1024 * 1024 {
        return Err(fail("resource budget exceeds 256 MiB"));
    }
    Ok((deck, resources))
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
