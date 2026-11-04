//! QPPT: a multi-file native project in a ZIP container. No format version field.
use crate::{import::Loss, *};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read, Write},
};

pub const ENTRY: &str = "deck.xml";
pub const MAX_BYTES: usize = 256 * 1024 * 1024;
const MAX_FILE: usize = 64 * 1024 * 1024;
const MAX_ENTRIES: usize = 4096;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: String,
    pub entry: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fonts: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub imported: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub losses: Vec<Loss>,
}
impl Default for Manifest {
    fn default() -> Self {
        Self {
            format: "qppt".into(),
            entry: ENTRY.into(),
            fonts: Vec::new(),
            imported: false,
            losses: Vec::new(),
        }
    }
}
impl Manifest {
    pub fn validate(&self) -> Result<()> {
        if self.format != "qppt" || self.entry != ENTRY {
            return Err(model::invalid(
                "QPPT requires format=qppt and entry=deck.xml",
            ));
        }
        if !self.imported && !self.losses.is_empty() {
            return Err(model::invalid("QPPT losses require imported=true"));
        }
        if self.fonts.len() > MAX_ENTRIES {
            return Err(model::invalid("QPPT has too many font references"));
        }
        Ok(())
    }
}

pub struct Project {
    pub manifest: Manifest,
    pub files: BTreeMap<String, String>,
    pub assets: BTreeMap<String, Vec<u8>>,
}

/// Normalize a project-relative name independently of the host OS.
pub fn path(name: &str) -> Result<String> {
    if name.is_empty()
        || name.len() > 256
        || name.starts_with('/')
        || name.contains(['\\', ':', '\0'])
    {
        return Err(model::invalid(
            "resources must use relative paths inside the project",
        ));
    }
    let parts = name.split('/').filter(|p| *p != ".").collect::<Vec<_>>();
    if parts.is_empty() || parts.iter().any(|p| p.is_empty() || *p == "..") {
        return Err(model::invalid(
            "resources must use relative paths inside the project",
        ));
    }
    Ok(parts.join("/"))
}

/// Require external page files, including for a one-page document.
pub fn require_project(source: &str) -> Result<()> {
    if source.len() > 4 * 1024 * 1024 || source.contains("<!DOCTYPE") || source.contains("<!ENTITY")
    {
        return Err(model::invalid("invalid or excessive QPPT entry source"));
    }
    let doc = roxmltree::Document::parse(source)?;
    if doc.root_element().tag_name().name() != "Deck" {
        return Err(model::invalid("QPPT entry must be a Deck"));
    }
    for node in doc
        .root_element()
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "Slide")
    {
        if node.attribute("src").is_none() {
            return Err(model::invalid(
                "QPPT uses a multi-file project: put every Slide in pages/ and reference it with src",
            ));
        }
    }
    Ok(())
}

fn media(deck: &Deck) -> BTreeSet<String> {
    fn visit(nodes: &[Node], deck: &Deck, out: &mut BTreeSet<String>) {
        for node in nodes {
            if let Kind::Image { src, .. } | Kind::Icon { src } = &node.kind {
                out.insert(deck.assets.get(src).unwrap_or(src).clone());
            }
            visit(&node.children, deck, out);
        }
    }
    let mut out = BTreeSet::new();
    for slide in &deck.slides {
        visit(&slide.nodes, deck, &mut out);
        if let Some(src) = layout::background_image(&slide.background) {
            out.insert(
                deck.assets
                    .get(src)
                    .map(String::as_str)
                    .unwrap_or(src)
                    .into(),
            );
        }
    }
    out
}

impl Project {
    /// Collect only referenced sources and media. The loader owns filesystem policy.
    pub fn load(
        entry: &str,
        manifest: Manifest,
        mut read: impl FnMut(&str) -> Result<Vec<u8>>,
    ) -> Result<Self> {
        manifest.validate()?;
        let source = String::from_utf8(read(entry)?)
            .map_err(|_| model::invalid("QPPT source must be UTF-8"))?;
        require_project(&source)?;
        let mut files = BTreeMap::new();
        let mut total = source.len();
        let deck = parse_project(ENTRY, &source, |name| {
            let text = String::from_utf8(read(name)?)
                .map_err(|_| model::invalid("QPPT source must be UTF-8"))?;
            total = total
                .checked_add(text.len())
                .ok_or_else(|| model::invalid("QPPT size overflow"))?;
            if total > MAX_BYTES {
                return Err(model::invalid("QPPT expanded size exceeds budget"));
            }
            files.insert(name.to_owned(), text.clone());
            Ok(text)
        })?;
        if files.insert(ENTRY.into(), source).is_some() {
            return Err(model::invalid(
                "deck.xml cannot also be a referenced source",
            ));
        }
        let mut assets = BTreeMap::new();
        for name in deck
            .fonts
            .iter()
            .map(|f| &f.src)
            .chain(&manifest.fonts)
            .chain(media(&deck).iter())
        {
            let name = path(name)?;
            if let std::collections::btree_map::Entry::Vacant(slot) = assets.entry(name) {
                let bytes = read(slot.key())?;
                total = total
                    .checked_add(bytes.len())
                    .ok_or_else(|| model::invalid("QPPT size overflow"))?;
                if bytes.len() > MAX_FILE || total > MAX_BYTES {
                    return Err(model::invalid("QPPT expanded size exceeds budget"));
                }
                slot.insert(bytes);
            }
        }
        let project = Self {
            manifest,
            files,
            assets,
        };
        project.validate()?;
        Ok(project)
    }
    pub fn deck(&self) -> Result<Deck> {
        self.manifest.validate()?;
        let source = self
            .files
            .get(ENTRY)
            .ok_or_else(|| model::invalid("QPPT is missing deck.xml"))?;
        require_project(source)?;
        parse_files(ENTRY, &self.files)
    }
    pub fn resources(&self, deck: &Deck) -> Result<Resources> {
        let mut used = 0usize;
        let mut get = |name: &str| -> Result<Vec<u8>> {
            let bytes = self
                .assets
                .get(&path(name)?)
                .ok_or_else(|| model::invalid(format!("missing QPPT resource {name}")))?;
            used = used
                .checked_add(bytes.len())
                .ok_or_else(|| model::invalid("QPPT resource size overflow"))?;
            if used > MAX_BYTES {
                return Err(model::invalid("QPPT resource budget exceeds 256 MiB"));
            }
            Ok(bytes.clone())
        };
        let mut resources = Resources::default();
        let mut loaded = BTreeSet::new();
        for font in &deck.fonts {
            resources.fonts.push(FontResource {
                family: Some(font.family.clone()),
                bytes: get(&font.src)?,
            });
            loaded.insert(path(&font.src)?);
        }
        for name in &self.manifest.fonts {
            if loaded.insert(path(name)?) {
                resources.fonts.push(FontResource {
                    family: None,
                    bytes: get(name)?,
                });
            }
        }
        for name in media(deck) {
            resources.images.insert(name.clone(), get(&name)?);
        }
        Ok(resources)
    }
    /// Move embedded bytes into the renderer; clone only multiply referenced assets.
    pub fn take_resources(&mut self, deck: &Deck) -> Result<Resources> {
        let mut fonts = deck
            .fonts
            .iter()
            .map(|f| (f.src.clone(), Some(f.family.clone())))
            .collect::<Vec<_>>();
        let mut loaded = fonts
            .iter()
            .map(|(n, _)| path(n))
            .collect::<Result<BTreeSet<_>>>()?;
        for name in &self.manifest.fonts {
            if loaded.insert(path(name)?) {
                fonts.push((name.clone(), None));
            }
        }
        let images = media(deck);
        let mut uses = BTreeMap::<String, usize>::new();
        for name in fonts.iter().map(|(n, _)| n).chain(images.iter()) {
            *uses.entry(path(name)?).or_default() += 1;
        }
        let used = uses.iter().try_fold(0usize, |sum, (name, count)| {
            let bytes = self
                .assets
                .get(name)
                .ok_or_else(|| model::invalid(format!("missing QPPT resource {name}")))?;
            bytes
                .len()
                .checked_mul(*count)
                .and_then(|n| sum.checked_add(n))
                .ok_or_else(|| model::invalid("QPPT resource size overflow"))
        })?;
        if used > MAX_BYTES {
            return Err(model::invalid("QPPT resource budget exceeds 256 MiB"));
        }
        let mut take = |name: &str| -> Result<Vec<u8>> {
            let name = path(name)?;
            let count = uses.get_mut(&name).expect("resource use was counted");
            *count -= 1;
            if *count == 0 {
                self.assets
                    .remove(&name)
                    .ok_or_else(|| model::invalid(format!("missing QPPT resource {name}")))
            } else {
                self.assets
                    .get(&name)
                    .cloned()
                    .ok_or_else(|| model::invalid(format!("missing QPPT resource {name}")))
            }
        };
        let mut resources = Resources::default();
        for (name, family) in fonts {
            resources.fonts.push(FontResource {
                family,
                bytes: take(&name)?,
            });
        }
        for name in images {
            resources.images.insert(name.clone(), take(&name)?);
        }
        Ok(resources)
    }
    /// Fonts supplied by the Agent's CLI become portable package assets.
    pub fn add_font(&mut self, bytes: Vec<u8>) -> Result<()> {
        if bytes.len() > MAX_FILE {
            return Err(model::invalid("QPPT font exceeds 64 MiB"));
        }
        let extension = if bytes.starts_with(b"ttcf") {
            "ttc"
        } else if bytes.starts_with(b"OTTO") {
            "otf"
        } else {
            "ttf"
        };
        let name = format!("fonts/{}.{}", digest(&bytes), extension);
        if !self.manifest.fonts.contains(&name) {
            self.manifest.fonts.push(name.clone());
        }
        self.assets.insert(name, bytes);
        self.validate()
    }
    pub fn validate(&self) -> Result<()> {
        let deck = self.deck()?;
        let mut names = BTreeSet::new();
        let manifest_bytes = serde_json::to_vec(&self.manifest)?.len();
        if manifest_bytes > 1024 * 1024 {
            return Err(model::invalid("QPPT manifest exceeds 1 MiB"));
        }
        let mut total = manifest_bytes;
        if self.files.len() + self.assets.len() + 1 > MAX_ENTRIES {
            return Err(model::invalid("QPPT has too many entries"));
        }
        for (name, bytes) in self
            .files
            .iter()
            .map(|(n, s)| (n, s.as_bytes()))
            .chain(self.assets.iter().map(|(n, b)| (n, b.as_slice())))
        {
            if path(name)? != *name
                || name == "manifest.json"
                || !names.insert(name.to_ascii_lowercase())
            {
                return Err(model::invalid("invalid or duplicate QPPT path"));
            }
            let max = if self.files.contains_key(name) {
                4 * 1024 * 1024
            } else {
                MAX_FILE
            };
            total = total
                .checked_add(bytes.len())
                .ok_or_else(|| model::invalid("QPPT size overflow"))?;
            if bytes.len() > max || total > MAX_BYTES {
                return Err(model::invalid("QPPT expanded size exceeds budget"));
            }
        }
        for name in self.files.keys().chain(self.assets.keys()) {
            let mut prefix = name.as_str();
            while let Some((parent, _)) = prefix.rsplit_once('/') {
                if names.contains(&parent.to_ascii_lowercase()) {
                    return Err(model::invalid("QPPT file/directory path collision"));
                }
                prefix = parent;
            }
        }
        for name in deck
            .fonts
            .iter()
            .map(|f| &f.src)
            .chain(&self.manifest.fonts)
            .chain(media(&deck).iter())
        {
            if !self.assets.contains_key(&path(name)?) {
                return Err(model::invalid(format!("missing QPPT resource {name}")));
            }
        }
        for name in media(&deck) {
            if self.assets[&path(&name)?].len() > 32 * 1024 * 1024 {
                return Err(model::invalid("QPPT media exceeds 32 MiB"));
            }
        }
        Ok(())
    }
    pub fn write(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let manifest = serde_json::to_vec(&self.manifest)?;
        if manifest.len() > 1024 * 1024 {
            return Err(model::invalid("QPPT manifest exceeds 1 MiB"));
        }
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o644);
        zip.start_file("manifest.json", options)?;
        zip.write_all(&manifest)?;
        for (name, bytes) in self
            .files
            .iter()
            .map(|(n, s)| (n, s.as_bytes()))
            .chain(self.assets.iter().map(|(n, b)| (n, b.as_slice())))
        {
            zip.start_file(name, options)?;
            zip.write_all(bytes)?;
        }
        let bytes = zip.finish()?.into_inner();
        if bytes.len() > MAX_BYTES {
            return Err(model::invalid("QPPT compressed size exceeds 256 MiB"));
        }
        Ok(bytes)
    }
    pub fn read(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_BYTES {
            return Err(model::invalid("QPPT compressed size exceeds 256 MiB"));
        }
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes))?;
        if zip.len() > MAX_ENTRIES {
            return Err(model::invalid("QPPT has too many entries"));
        }
        let mut entries = BTreeMap::new();
        let mut names = BTreeSet::new();
        let mut total = 0usize;
        for i in 0..zip.len() {
            let mut file = zip.by_index(i)?;
            let name = file.name().to_owned();
            let normalized = path(name.trim_end_matches('/'))?;
            if normalized != name.trim_end_matches('/')
                || !names.insert(normalized.to_ascii_lowercase())
            {
                return Err(model::invalid("invalid or duplicate QPPT ZIP path"));
            }
            if file.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
                return Err(model::invalid("QPPT symlinks are not allowed"));
            }
            if file.is_dir() {
                continue;
            }
            let max = if name == "manifest.json" {
                1024 * 1024
            } else {
                MAX_FILE
            };
            if file.size() > max as u64 {
                return Err(model::invalid("QPPT ZIP entry exceeds budget"));
            }
            let mut data = Vec::new();
            file.by_ref().take(max as u64 + 1).read_to_end(&mut data)?;
            total = total
                .checked_add(data.len())
                .ok_or_else(|| model::invalid("QPPT size overflow"))?;
            if data.len() > max || total > MAX_BYTES {
                return Err(model::invalid("QPPT expanded size exceeds budget"));
            }
            entries.insert(name, data);
        }
        let manifest = entries
            .remove("manifest.json")
            .ok_or_else(|| model::invalid("not a QPPT document: missing manifest.json"))?;
        let manifest: Manifest = serde_json::from_slice(&manifest)?;
        Self::load(ENTRY, manifest, |name| {
            entries
                .remove(name)
                .ok_or_else(|| model::invalid(format!("missing QPPT file {name}")))
        })
    }
}
