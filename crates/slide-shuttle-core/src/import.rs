//! Bounded package inspection and explicitly lossy reconstruction into our own format.
mod colors;
mod styles;
use crate::{
    model::*,
    pptx::{A, P, R},
};
use colors::{Problems, color_hex};
use roxmltree::{Document, Node as X};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
};
use styles::Styles;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Loss {
    pub page: String,
    pub object: String,
    pub reason: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Object {
    pub page: String,
    pub name: String,
    pub native_id: String,
    pub kind: String,
    pub parent: Option<String>,
    pub geometry: [i64; 4],
    pub text: String,
    pub style: Value,
    pub payload: String,
    pub native_digest: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub version: u32,
    pub digest: String,
    pub export_id: Option<String>,
    pub width: i64,
    pub height: i64,
    pub pages: Vec<String>,
    pub objects: Vec<Object>,
    pub inventory: BTreeMap<String, String>,
    pub losses: Vec<Loss>,
}
pub struct Imported {
    /// Project sources: `deck.xml` plus one `pages/<slide id>.xml` per slide.
    pub files: BTreeMap<String, String>,
    pub assets: BTreeMap<String, Vec<u8>>,
    pub snapshot: Snapshot,
}

pub(crate) fn read_package(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>> {
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(invalid("PPTX compressed size exceeds 64 MiB"));
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))?;
    if zip.len() > 4096 {
        return Err(invalid("PPTX has too many entries"));
    }
    let mut total = 0u64;
    let mut entries = BTreeMap::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i)?;
        let name = f.name().to_owned();
        if f.is_dir() {
            continue;
        }
        if name.starts_with('/')
            || name.contains('\\')
            || name
                .split('/')
                .any(|x| x == ".." || x == "." || x.is_empty())
            || name.contains('\0')
            || name.contains(':')
        {
            return Err(invalid("unsafe ZIP path"));
        }
        if f.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            return Err(invalid("ZIP symlinks are not allowed"));
        }
        total = total
            .checked_add(f.size())
            .ok_or_else(|| invalid("ZIP size overflow"))?;
        if f.size() > 64 * 1024 * 1024 || total > 256 * 1024 * 1024 {
            return Err(invalid("PPTX expanded size exceeds budget"));
        }
        let mut data = Vec::new();
        f.by_ref()
            .take(64 * 1024 * 1024 + 1)
            .read_to_end(&mut data)?;
        if data.len() > 64 * 1024 * 1024 {
            return Err(invalid("ZIP entry size exceeds budget"));
        }
        if entries.insert(name, data).is_some() {
            return Err(invalid("duplicate ZIP entry"));
        }
    }
    if !entries.contains_key("[Content_Types].xml") || !entries.contains_key("ppt/presentation.xml")
    {
        return Err(invalid("not an unencrypted PPTX presentation"));
    }
    // Validate every XML entry before any potentially expensive traversal.
    for (name, bytes) in &entries {
        if name.ends_with(".xml") || name.ends_with(".rels") {
            let doc = parse_xml(bytes)?;
            if doc.descendants().count() > 250_000 {
                return Err(invalid("XML node budget exceeded"));
            }
        }
    }
    Ok(entries)
}
fn parse_xml(bytes: &[u8]) -> Result<Document<'_>> {
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(invalid("XML entry exceeds 16 MiB"));
    }
    let source = std::str::from_utf8(bytes).map_err(|_| invalid("PPTX XML is not UTF-8"))?;
    if source.contains("<!DOCTYPE") || source.contains("<!ENTITY") {
        return Err(invalid("DTD/entities are forbidden"));
    }
    Ok(Document::parse(source)?)
}
pub(crate) fn resolve_part(part: &str, target: &str) -> Result<String> {
    if target.contains('\\') || target.contains(':') || target.contains('\0') {
        return Err(invalid("invalid relationship target"));
    }
    let mut components = if target.starts_with('/') {
        Vec::new()
    } else {
        part.split('/')
            .take(part.split('/').count().saturating_sub(1))
            .collect::<Vec<_>>()
    };
    for c in target.trim_start_matches('/').split('/') {
        match c {
            "" | "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(invalid("relationship escapes package"));
                }
            }
            _ => components.push(c),
        }
    }
    Ok(components.join("/"))
}
fn rel_path(part: &str) -> String {
    let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
    format!("{dir}/_rels/{file}.rels")
}
const HYPERLINK: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";
fn hyperlinks(entries: &BTreeMap<String, Vec<u8>>, part: &str) -> Result<BTreeMap<String, String>> {
    let Some(bytes) = entries.get(&rel_path(part)) else {
        return Ok(BTreeMap::new());
    };
    let doc = parse_xml(bytes)?;
    Ok(doc
        .root_element()
        .children()
        .filter(|r| {
            r.attribute("TargetMode") == Some("External") && r.attribute("Type") == Some(HYPERLINK)
        })
        .filter_map(|r| Some((r.attribute("Id")?.into(), r.attribute("Target")?.into())))
        .collect())
}
fn rels(entries: &BTreeMap<String, Vec<u8>>, part: &str) -> Result<BTreeMap<String, String>> {
    let Some(bytes) = entries.get(&rel_path(part)) else {
        return Ok(BTreeMap::new());
    };
    let doc = parse_xml(bytes)?;
    let mut out = BTreeMap::new();
    for r in doc.root_element().children().filter(|x| x.is_element()) {
        if r.attribute("TargetMode") == Some("External") {
            continue;
        }
        if let (Some(id), Some(target)) = (r.attribute("Id"), r.attribute("Target")) {
            out.insert(id.into(), resolve_part(part, target)?);
        }
    }
    Ok(out)
}
fn child<'a, 'i>(n: X<'a, 'i>, ns: &str, name: &str) -> Option<X<'a, 'i>> {
    n.children().find(|x| x.has_tag_name((ns, name)))
}
fn desc<'a, 'i>(n: X<'a, 'i>, ns: &str, name: &str) -> Option<X<'a, 'i>> {
    n.descendants().find(|x| x.has_tag_name((ns, name)))
}
fn related_part(
    entries: &BTreeMap<String, Vec<u8>>,
    part: &str,
    kind: &str,
) -> Result<Option<String>> {
    let Some(bytes) = entries.get(&rel_path(part)) else {
        return Ok(None);
    };
    let doc = parse_xml(bytes)?;
    for relationship in doc.root_element().children().filter(|n| n.is_element()) {
        if relationship.attribute("TargetMode") == Some("External") {
            continue;
        }
        if relationship
            .attribute("Type")
            .is_some_and(|t| t.rsplit('/').next() == Some(kind))
            && let Some(target) = relationship.attribute("Target")
        {
            return resolve_part(part, target).map(Some);
        }
    }
    Ok(None)
}
fn number(n: Option<X<'_, '_>>, key: &str, default: i64) -> i64 {
    n.and_then(|x| x.attribute(key))
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
fn plain(n: X<'_, '_>) -> String {
    let paragraphs = n
        .descendants()
        .filter(|x| x.has_tag_name((A, "p")))
        .collect::<Vec<_>>();
    paragraphs
        .into_iter()
        .map(|p| {
            let mut text = String::new();
            for n in p.descendants() {
                if n.has_tag_name((A, "t")) {
                    text.push_str(n.text().unwrap_or(""));
                } else if n.has_tag_name((A, "br")) {
                    text.push('\n');
                }
            }
            text
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn canonical(n: X<'_, '_>) -> Value {
    let mut attrs = BTreeMap::new();
    for a in n.attributes() {
        if a.name() != "dirty" {
            attrs.insert(
                format!("{}:{}", a.namespace().unwrap_or(""), a.name()),
                a.value(),
            );
        }
    }
    let children = n
        .children()
        .filter(|c| c.is_element())
        .map(canonical)
        .collect::<Vec<_>>();
    let texts = n
        .children()
        .filter(|c| c.is_text())
        .filter_map(|c| c.text())
        .filter(|s| !s.trim().is_empty())
        .collect::<Vec<_>>();
    json!({"ns":n.tag_name().namespace(),"tag":n.tag_name().name(),"attrs":attrs,"text":texts,"children":children})
}
fn semantic_hash(bytes: &[u8]) -> Result<String> {
    Ok(digest(&serde_json::to_vec(&canonical(
        parse_xml(bytes)?.root_element(),
    ))?))
}
fn effective_xfrm<'a>(n: X<'a, 'a>, fallback: &'a [Document<'a>]) -> Option<X<'a, 'a>> {
    if let Some(x) = desc(n, A, "xfrm").or_else(|| child(n, P, "xfrm")) {
        return Some(x);
    }
    let ph = desc(n, P, "ph")?;
    let idx = ph.attribute("idx").unwrap_or("0");
    let typ = ph.attribute("type");
    for doc in fallback {
        for shape in doc.descendants().filter(|x| x.has_tag_name((P, "sp"))) {
            if let Some(p) = desc(shape, P, "ph")
                && p.attribute("idx").unwrap_or("0") == idx
                && (typ.is_none() || p.attribute("type") == typ)
                && let Some(x) = desc(shape, A, "xfrm")
            {
                return Some(x);
            }
        }
    }
    None
}
#[derive(Clone, Copy)]
struct Transform {
    sx: f64,
    sy: f64,
    tx: f64,
    ty: f64,
}
impl Transform {
    fn rect(self, r: [i64; 4]) -> [i64; 4] {
        [
            (r[0] as f64 * self.sx + self.tx).round() as i64,
            (r[1] as f64 * self.sy + self.ty).round() as i64,
            (r[2] as f64 * self.sx).round() as i64,
            (r[3] as f64 * self.sy).round() as i64,
        ]
    }
}
/// Per-slide inputs shared by every object in the slide tree.
#[derive(Clone, Copy)]
struct SlidePart<'d, 'i, 'a> {
    page: &'a str,
    rels: &'a BTreeMap<String, String>,
    /// External hyperlink targets by relationship id.
    links: &'a BTreeMap<String, String>,
    fallback: &'d [Document<'i>],
    styles: &'a Styles<'d, 'i>,
}
/// The reconstructed object's identity, position and translated style.
struct Target<'a> {
    name: &'a str,
    id: &'a str,
    pos: &'a str,
    css: &'a str,
    props: &'a Value,
}
struct Reader<'a> {
    entries: &'a BTreeMap<String, Vec<u8>>,
    objects: Vec<Object>,
    losses: Vec<Loss>,
    assets: BTreeMap<String, Vec<u8>>,
    nodes: Vec<String>,
}

pub fn inspect_pptx(bytes: &[u8]) -> Result<Snapshot> {
    Ok(import_pptx(bytes)?.snapshot)
}
pub fn import_pptx(bytes: &[u8]) -> Result<Imported> {
    let entries = read_package(bytes)?;
    let presentation = parse_xml(&entries["ppt/presentation.xml"])?;
    let size = desc(presentation.root_element(), P, "sldSz");
    let (width, height) = (number(size, "cx", 12192000), number(size, "cy", 6858000));
    if width <= 0 || height <= 0 || width > 156057600 || height > 156057600 {
        return Err(invalid("PPTX page dimensions exceed supported range"));
    }
    let pr = rels(&entries, "ppt/presentation.xml")?;
    let mut paths = Vec::new();
    for s in presentation
        .descendants()
        .filter(|n| n.has_tag_name((P, "sldId")))
    {
        let id = s
            .attribute((R, "id"))
            .ok_or_else(|| invalid("slide has no relationship"))?;
        paths.push(
            pr.get(id)
                .ok_or_else(|| invalid("missing slide relationship"))?
                .clone(),
        );
    }
    if paths.is_empty() || paths.len() > 300 {
        return Err(invalid("PPTX requires 1–300 slides"));
    }
    let mut reader = Reader {
        entries: &entries,
        objects: Vec::new(),
        losses: Vec::new(),
        assets: BTreeMap::new(),
        nodes: Vec::new(),
    };
    let mut slides = Vec::new();
    let mut pages = Vec::new();
    let mut inventory = BTreeMap::new();
    for n in presentation
        .root_element()
        .children()
        .filter(|n| n.is_element())
    {
        let tag = n.tag_name().name();
        if ![
            "sldMasterIdLst",
            "notesMasterIdLst",
            "sldIdLst",
            "sldSz",
            "notesSz",
        ]
        .contains(&tag)
        {
            inventory.insert(
                format!("ppt/presentation.xml#{tag}"),
                digest(&serde_json::to_vec(&canonical(n))?),
            );
            if tag == "defaultTextStyle" {
                continue;
            }
            reader.losses.push(Loss {
                page: "package".into(),
                object: tag.into(),
                reason: "presentation-level content is not reconstructed".into(),
            });
        }
    }
    let mut default_text_problems = Problems::new();
    if let Some(bytes) = entries.get("docProps/custom.xml") {
        let custom = parse_xml(bytes)?;
        for n in custom
            .root_element()
            .children()
            .filter(|n| n.is_element() && n.attribute("name") != Some("slideShuttleExportId"))
        {
            let name = n.attribute("name").unwrap_or("unknown");
            inventory.insert(
                format!("docProps/custom.xml#{name}"),
                digest(&serde_json::to_vec(&canonical(n))?),
            );
            reader.losses.push(Loss {
                page: "package".into(),
                object: name.into(),
                reason: "custom metadata is not reconstructed".into(),
            });
        }
    }
    for (i, path) in paths.iter().enumerate() {
        let data = entries
            .get(path)
            .ok_or_else(|| invalid("missing slide part"))?;
        let doc = parse_xml(data)?;
        let root = doc.root_element();
        let page = format!("page-{}", i + 1);
        if let Some(bg) = desc(root, P, "bg") {
            inventory.insert(
                format!("{path}#background"),
                digest(&serde_json::to_vec(&canonical(bg))?),
            );
        }
        pages.push(page.clone());
        let r = rels(&entries, path)?;
        let links = hyperlinks(&entries, path)?;
        let mut fallback_bytes = Vec::new();
        let mut style_parts = vec![path.clone()];
        if let Some(layout) = related_part(&entries, path, "slideLayout")? {
            style_parts.push(layout.clone());
            if let Some(b) = entries.get(&layout) {
                fallback_bytes.push(b.as_slice());
            }
            if let Some(master) = related_part(&entries, &layout, "slideMaster")?
                && let Some(b) = entries.get(&master)
            {
                style_parts.push(master);
                fallback_bytes.push(b.as_slice());
            }
        }
        let fallback = fallback_bytes
            .iter()
            .map(|b| parse_xml(b))
            .collect::<Result<Vec<_>>>()?;
        let styles = Styles::new(
            &entries,
            presentation.root_element(),
            root,
            &fallback,
            &style_parts,
        )?;
        default_text_problems.extend(styles.default_text_limits());
        // Master/layout decorative shapes are not silently treated as empty.
        for f in &fallback {
            let count = f
                .descendants()
                .filter(|x| x.has_tag_name((P, "sp")) || x.has_tag_name((P, "pic")))
                .filter(|x| desc(*x, P, "ph").is_none())
                .count();
            if count > 0 {
                reader.losses.push(Loss {
                    page: page.clone(),
                    object: "master/layout".into(),
                    reason: format!(
                        "{count} inherited decorative objects require explicit reconstruction"
                    ),
                });
            }
        }
        reader.nodes.clear();
        if let Some(tree) = desc(root, P, "spTree") {
            let part = SlidePart {
                page: &page,
                rels: &r,
                links: &links,
                fallback: &fallback,
                styles: &styles,
            };
            reader.walk(
                tree,
                &part,
                Transform {
                    sx: 1.0,
                    sy: 1.0,
                    tx: 0.0,
                    ty: 0.0,
                },
                None,
                0,
            )?;
        }
        // Office 2010+ wraps transitions in AlternateContent; its Fallback is plain OOXML.
        let transition = child(root, P, "transition").or_else(|| {
            root.children()
                .filter(|n| n.tag_name().name() == "AlternateContent")
                .flat_map(|n| n.children())
                .filter(|n| n.tag_name().name() == "Fallback")
                .find_map(|n| child(n, P, "transition"))
        });
        let mut show = Problems::new();
        let mut attrs = String::new();
        let mut extra = Vec::new();
        for (feature, node) in [
            ("timing", child(root, P, "timing")),
            ("transition", transition),
            ("extLst", child(root, P, "extLst")),
        ] {
            let Some(n) = node else { continue };
            inventory.insert(
                format!("{path}#{feature}"),
                digest(&serde_json::to_vec(&canonical(n))?),
            );
            match feature {
                "transition" => {
                    if let Some(t) = crate::show::import_transition(n, &mut show) {
                        attrs.push_str(&format!(" transition=\"{}\"", xml(&t)));
                    }
                }
                "timing" => {
                    // Each native shape id -> the elements it became, in order.
                    let joined = reader.nodes.join("");
                    let emitted = joined
                        .split(" id=\"")
                        .skip(1)
                        .filter_map(|s| s.split('"').next())
                        .collect::<Vec<_>>();
                    let mut targets = BTreeMap::new();
                    // Import ids number objects across the whole deck. Groups are flattened, so
                    // a group's animation plays on every member.
                    let objects = reader
                        .objects
                        .iter()
                        .enumerate()
                        .filter(|(_, o)| o.page == page)
                        .collect::<Vec<_>>();
                    let inside = |o: &Object, group: &str| {
                        let mut parent = o.parent.clone();
                        while let Some(p) = parent {
                            if p == group {
                                return true;
                            }
                            parent = objects
                                .iter()
                                .find(|(_, x)| x.kind == "grpSp" && x.name == p)
                                .and_then(|(_, x)| x.parent.clone());
                        }
                        false
                    };
                    for (_, o) in &objects {
                        let ids = objects
                            .iter()
                            .filter(|(_, x)| {
                                std::ptr::eq(*x, *o) || o.kind == "grpSp" && inside(x, &o.name)
                            })
                            .flat_map(|(i, _)| {
                                let base = format!("{page}/object-{}", i + 1);
                                emitted
                                    .iter()
                                    .filter(move |id| {
                                        **id == base || id.starts_with(&format!("{base}/"))
                                    })
                                    .map(|id| (*id).to_owned())
                            })
                            .collect::<Vec<_>>();
                        if !ids.is_empty() {
                            targets.entry(o.native_id.clone()).or_insert(ids);
                        }
                    }
                    let animated_group = n
                        .descendants()
                        .filter(|t| t.tag_name().name() == "spTgt")
                        .filter_map(|t| t.attribute("spid"))
                        .any(|spid| {
                            objects
                                .iter()
                                .any(|(_, o)| o.kind == "grpSp" && o.native_id == spid)
                        });
                    if animated_group {
                        show.insert("a group's animation plays on each member".into());
                    }
                    extra.extend(crate::show::import_timing(
                        n,
                        &targets,
                        (
                            (width as f64 / EMU_PER_PX) as f32,
                            (height as f64 / EMU_PER_PX) as f32,
                        ),
                        &mut show,
                    ));
                }
                _ => {
                    show.insert(format!(
                        "{feature} is retained in the inventory but is not reconstructed"
                    ));
                }
            }
        }
        for reason in show {
            reader.losses.push(Loss {
                page: page.clone(),
                object: "slide show".into(),
                reason,
            });
        }
        if let Some(layout) = related_part(&entries, path, "slideLayout")?
            && let Some(bytes) = entries.get(&layout)
        {
            let doc = parse_xml(bytes)?;
            let l = doc.root_element();
            let name = desc(l, P, "cSld").and_then(|c| c.attribute("name"));
            let kind = match (l.attribute("type"), name) {
                (Some("title"), _) => Some("cover"),
                (Some("secHead"), _) => Some("section"),
                (Some("obj"), _) => Some("content"),
                (Some("cust") | None, Some("Contents")) => Some("contents"),
                (Some("cust") | None, Some("End")) => Some("end"),
                _ => None,
            };
            if let Some(k) = kind {
                attrs.push_str(&format!(" type=\"{k}\""));
            }
        }
        if let Some(notes) = related_part(&entries, path, "notesSlide")?
            && let Some(bytes) = entries.get(&notes)
        {
            let lines = crate::show::import_notes(parse_xml(bytes)?.root_element());
            if !lines.is_empty() {
                extra.push(format!("<Notes>{}</Notes>", xml(&lines.join("\n"))));
            }
        }
        let mut problems = Problems::new();
        // A slide's own picture background becomes url(<asset>); inherited ones stay losses.
        let picture = desc(root, P, "bg")
            .and_then(|bg| child(bg, P, "bgPr"))
            .and_then(|pr| child(pr, A, "blipFill"))
            .and_then(|fill| {
                let bytes = child(fill, A, "blip")
                    .and_then(|b| b.attribute((R, "embed")))
                    .and_then(|id| r.get(id))
                    .and_then(|path| entries.get(path))?;
                let (w, h) = crate::render::image_size(bytes).ok()?;
                let page_ratio = width as f64 / height as f64;
                if child(fill, A, "srcRect").is_none()
                    && ((w as f64 / h as f64) / page_ratio - 1.0).abs() > 0.01
                {
                    problems.insert("stretched background image is drawn cropped to cover".into());
                }
                let asset = format!("media/{}.png", digest(bytes));
                reader.assets.insert(asset.clone(), bytes.clone());
                Some(format!("url({asset})"))
            });
        let bg = match picture {
            Some(url) => url,
            None => styles.background(&mut problems),
        };
        for reason in problems {
            reader.losses.push(Loss {
                page: page.clone(),
                object: "background".into(),
                reason,
            });
        }
        slides.push((
            page.clone(),
            format!(
                "<Slide id=\"{page}\" background=\"{}\"{attrs}>{}{}</Slide>",
                xml(&bg),
                reader.nodes.join(""),
                extra.concat()
            ),
        ));
    }
    for reason in default_text_problems {
        reader.losses.push(Loss {
            page: "package".into(),
            object: "defaultTextStyle".into(),
            reason,
        });
    }
    for (name, data) in &entries {
        if paths.contains(name) || name == "docProps/custom.xml" || name == "ppt/presentation.xml" {
            continue;
        }
        let hash = if name.ends_with(".xml") {
            semantic_hash(data)?
        } else {
            digest(data)
        };
        inventory.insert(name.clone(), hash);
        if name.contains("/comments")
            || name.contains("/diagrams/")
            || name.contains("/activeX/")
            || name.contains("/embeddings/") && !name.ends_with(".xlsx")
        {
            reader.losses.push(Loss {
                page: "package".into(),
                object: name.clone(),
                reason: "native package content is not reconstructed by import".into(),
            });
        }
    }
    for (name, data) in entries.iter().filter(|(n, _)| n.ends_with(".rels")) {
        let doc = parse_xml(data)?;
        for rel in doc.root_element().children().filter(|n| n.is_element()) {
            if rel.attribute("TargetMode") == Some("External") {
                // Hyperlinks are addresses, rebuilt as <A href>; anything else would be fetched.
                if rel.attribute("Type") != Some(HYPERLINK) {
                    reader.losses.push(Loss {
                        page: "package".into(),
                        object: name.clone(),
                        reason: format!(
                            "external relationship not fetched: {}",
                            rel.attribute("Type").unwrap_or("unknown")
                        ),
                    });
                }
                inventory.insert(
                    format!("{name}#external:{}", rel.attribute("Id").unwrap_or("")),
                    digest(&serde_json::to_vec(&canonical(rel))?),
                );
            }
        }
    }
    let export_id = entries
        .get("docProps/custom.xml")
        .and_then(|b| parse_xml(b).ok())
        .and_then(|d| {
            d.descendants()
                .find(|n| n.attribute("name") == Some("slideShuttleExportId"))
                .map(|n| {
                    n.descendants()
                        .filter(|n| n.is_text())
                        .filter_map(|n| n.text())
                        .collect::<String>()
                })
        });
    let asset_xml = reader
        .assets
        .keys()
        .map(|p| format!("<Asset id=\"{}\" src=\"{}\"/>", xml(p), xml(p)))
        .collect::<String>();
    let mut files = BTreeMap::new();
    let mut pages_xml = String::new();
    for (id, slide) in slides {
        let path = format!("pages/{id}.xml");
        pages_xml.push_str(&format!("<Slide src=\"{path}\"/>"));
        files.insert(path, slide);
    }
    files.insert(
        "deck.xml".into(),
        format!(
            "<Deck title=\"Imported presentation\" width=\"{width}emu\" height=\"{height}emu\" font-family=\"Noto Sans CJK SC\">{asset_xml}{pages_xml}</Deck>"
        ),
    );
    let snapshot = Snapshot {
        version: 1,
        digest: digest(bytes),
        export_id,
        width,
        height,
        pages,
        objects: reader.objects,
        inventory,
        losses: reader.losses,
    };
    Ok(Imported {
        files,
        assets: reader.assets,
        snapshot,
    })
}

impl Reader<'_> {
    fn loss(&mut self, page: &str, object: &str, reason: impl Into<String>) {
        self.losses.push(Loss {
            page: page.into(),
            object: object.into(),
            reason: reason.into(),
        });
    }
    fn walk<'d, 'i>(
        &mut self,
        tree: X<'d, 'i>,
        part: &SlidePart<'d, 'i, '_>,
        transform: Transform,
        parent: Option<String>,
        depth: usize,
    ) -> Result<()> {
        let SlidePart {
            page,
            rels,
            fallback,
            styles,
            ..
        } = *part;
        if depth > 32 {
            return Err(invalid("group nesting exceeds limit"));
        }
        for n in tree.children().filter(|n| n.is_element()) {
            let tag = n.tag_name().name();
            if ["nvGrpSpPr", "grpSpPr"].contains(&tag) {
                continue;
            }
            if !["sp", "pic", "graphicFrame", "cxnSp", "grpSp"].contains(&tag) {
                self.loss(page, tag, "unsupported slide tree element");
                continue;
            }
            let nv = desc(n, P, "cNvPr");
            let native_id = nv.and_then(|v| v.attribute("id")).unwrap_or("").to_owned();
            let name = nv
                .and_then(|v| v.attribute("name"))
                .unwrap_or("")
                .to_owned();
            let import_id = format!("{page}/object-{}", self.objects.len() + 1);
            let xf = if tag == "grpSp" {
                child(n, P, "grpSpPr").and_then(|n| child(n, A, "xfrm"))
            } else {
                effective_xfrm(n, fallback)
            };
            let off = xf.and_then(|n| child(n, A, "off"));
            let ext = xf.and_then(|n| child(n, A, "ext"));
            let local = [
                number(off, "x", 0),
                number(off, "y", 0),
                number(ext, "cx", 0),
                number(ext, "cy", 0),
            ];
            let geometry = transform.rect(local);
            if xf.is_none() {
                self.loss(page, &name, "missing effective geometry");
            }
            let rot = xf
                .and_then(|x| x.attribute("rot"))
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0)
                / 60000.0;
            let flip = |k| {
                xf.and_then(|x| x.attribute(k))
                    .is_some_and(|v| v == "1" || v == "true")
            };
            let turned = rot != 0.0 || flip("flipH") || flip("flipV");
            let raw_text = plain(n);
            let text = if tag == "grpSp" {
                String::new()
            } else {
                raw_text
            };
            let mut problems = Problems::new();
            let props = styles.properties(n, &mut problems);
            let payload = payload(n, rels, self.entries)?;
            let native_digest = digest(&serde_json::to_vec(&canonical(n))?);
            self.objects.push(Object {
                page: page.into(),
                name: name.clone(),
                native_id,
                kind: tag.into(),
                parent: parent.clone(),
                geometry,
                text: text.clone(),
                style: props.clone(),
                payload,
                native_digest,
            });
            // The shape's own fill, line and effects are rebuilt by the style resolver.
            let own = |x: X<'_, '_>| {
                x.parent().is_some_and(|p| {
                    p.has_tag_name((P, "spPr")) && p.parent() == Some(n)
                        || p.has_tag_name((A, "ln"))
                            && p.parent().and_then(|q| q.parent()) == Some(n)
                })
            };
            for feature in ["gradFill", "effectDag", "blip"] {
                let unsupported = n.descendants().any(|x| {
                    x.has_tag_name((A, feature))
                        && match feature {
                            "gradFill" => !own(x),
                            "effectDag" => true,
                            "blip" => x
                                .children()
                                .any(|c| c.is_element() && !c.has_tag_name((A, "alphaModFix"))),
                            _ => false,
                        }
                });
                if unsupported {
                    self.loss(
                        page,
                        &name,
                        format!("{feature} appearance is not reconstructed"),
                    );
                }
            }
            if tag == "grpSp" {
                self.loss(
                    page,
                    &name,
                    "native group hierarchy flattened into absolute objects",
                );
                let choff = xf.and_then(|n| child(n, A, "chOff"));
                let chext = xf.and_then(|n| child(n, A, "chExt"));
                let cw = number(chext, "cx", local[2]).max(1) as f64;
                let ch = number(chext, "cy", local[3]).max(1) as f64;
                let sx = transform.sx * local[2] as f64 / cw;
                let sy = transform.sy * local[3] as f64 / ch;
                let next = Transform {
                    sx,
                    sy,
                    tx: geometry[0] as f64 - number(choff, "x", 0) as f64 * sx,
                    ty: geometry[1] as f64 - number(choff, "y", 0) as f64 * sy,
                };
                self.walk(n, part, next, Some(name.clone()), depth + 1)?;
                continue;
            }
            let pos = format!(
                "position:absolute;left:{}emu;top:{}emu;width:{}emu;height:{}emu",
                geometry[0],
                geometry[1],
                geometry[2].max(0),
                geometry[3].max(0)
            );
            let mut props = props;
            let has_text = !text.trim().is_empty();
            let boxed = props["fill"]
                .as_str()
                .is_some_and(|f| f.len() == 6 || !f.ends_with("00"))
                || props["gradient"].is_string()
                || props["strokeWidth"].as_u64().is_some_and(|w| w > 0);
            if let Some(obj) = props.as_object_mut() {
                if tag == "graphicFrame" {
                    for (key, what) in [("gradient", "gradient"), ("shadow", "shadow")] {
                        if obj.remove(key).is_some() {
                            self.loss(
                                page,
                                &name,
                                format!("table/chart {what} is not reconstructed"),
                            );
                        }
                    }
                }
                if has_text && !boxed && obj.remove("shadow").is_some() {
                    self.loss(page, &name, "text shadow is not reconstructed");
                }
            }
            let mut css = style_css(&props, false);
            if turned {
                // Text in a non-rectangular preset gets its box from a Shape behind it.
                let shaped = desc(n, A, "prstGeom")
                    .and_then(preset_shape)
                    .is_some_and(|(p, _)| p != "rect");
                if tag == "grpSp" || tag == "graphicFrame" || has_text && boxed && !shaped {
                    self.loss(page, &name, "rotation/flip is not reconstructed");
                } else {
                    css.push_str(&format!(
                        ";transform:rotate({}deg){}{}",
                        (rot * 100.0).round() / 100.0,
                        if flip("flipH") { " scaleX(-1)" } else { "" },
                        if flip("flipV") { " scaleY(-1)" } else { "" }
                    ));
                }
            }
            if let Some(geom) = desc(n, A, "prstGeom")
                && geom.attribute("prst") == Some("roundRect")
            {
                let adj = geom
                    .descendants()
                    .find(|g| g.has_tag_name((A, "gd")) && g.attribute("name") == Some("adj"))
                    .and_then(|g| g.attribute("fmla"))
                    .and_then(|s| s.strip_prefix("val "))
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(16667.0);
                if adj.is_finite() && (0.0..=50000.0).contains(&adj) {
                    css.push_str(&format!(
                        ";border-radius:{}emu",
                        geometry[2].min(geometry[3]) as f64 * adj / 100000.0
                    ));
                } else {
                    self.loss(
                        page,
                        &name,
                        "rounded corner guide could not be reconstructed",
                    );
                }
            }
            let target = Target {
                name: &name,
                id: &import_id,
                pos: &pos,
                css: &css,
                props: &props,
            };
            let out = self.reconstruct(n, part, &target, &mut problems)?;
            if !out.is_empty() {
                self.nodes.push(out);
            }
            for reason in problems {
                self.loss(page, &name, reason);
            }
        }
        Ok(())
    }
    /// One table cell: spans, padding, anchor, fill, the four borders and its rich text.
    fn table_cell(
        &self,
        n: X<'_, '_>,
        tc: X<'_, '_>,
        part: &SlidePart<'_, '_, '_>,
        problems: &mut Problems,
    ) -> String {
        let styles = part.styles;
        let mut attrs = String::new();
        for (ooxml, ours) in [("gridSpan", "colspan"), ("rowSpan", "rowspan")] {
            if let Some(v) = tc.attribute(ooxml).filter(|v| *v != "1") {
                attrs.push_str(&format!(" {ours}=\"{}\"", xml(v)));
            }
        }
        let pr = child(tc, A, "tcPr");
        let emu = |k: &str, default: i64| {
            pr.and_then(|p| p.attribute(k))
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(default)
        };
        let mut css = vec![format!(
            "padding:{}emu {}emu {}emu {}emu",
            emu("marT", 45720),
            emu("marR", 91440),
            emu("marB", 45720),
            emu("marL", 91440)
        )];
        css.push(format!(
            "vertical-align:{}",
            match pr.and_then(|p| p.attribute("anchor")) {
                Some("ctr") => "middle",
                Some("b") => "bottom",
                _ => "top",
            }
        ));
        if let Some(pr) = pr {
            if let Some(color) = styles.theme.fill(pr, None, problems).filter(|c| c.3 > 0) {
                css.push(format!("background-color:#{}", color_hex(color)));
            }
            for (tag, side) in [
                ("lnT", "top"),
                ("lnR", "right"),
                ("lnB", "bottom"),
                ("lnL", "left"),
            ] {
                let Some(ln) = child(pr, A, tag) else {
                    continue;
                };
                let width = ln
                    .attribute("w")
                    .and_then(|w| w.parse::<i64>().ok())
                    .unwrap_or(12700);
                match styles
                    .theme
                    .fill(ln, None, problems)
                    .filter(|c| c.3 > 0 && width > 0)
                {
                    Some(color) => {
                        let dash = match child(ln, A, "prstDash").and_then(|d| d.attribute("val")) {
                            Some("sysDot" | "dot") => "dotted",
                            Some("solid") | None => "solid",
                            _ => "dashed",
                        };
                        css.push(format!(
                            "border-{side}:{width}emu {dash} #{}",
                            color_hex(color)
                        ));
                    }
                    None => css.push(format!("border-{side}-width:0")),
                }
            }
        }
        let body = child(tc, A, "txBody");
        let first = body.and_then(|b| b.children().find(|x| x.has_tag_name((A, "p"))));
        let base = styles.text_properties(n, first, None, problems);
        if let Some(align) = base
            .get("align")
            .and_then(Value::as_str)
            .filter(|a| *a != "l")
        {
            css.push(format!("text-align:{}", text_align(align)));
        }
        let content = body
            .filter(|b| !plain(*b).trim().is_empty())
            .map(|b| self.rich_body(n, b, &Value::Object(base), part, problems))
            .unwrap_or_default();
        format!(
            "<Cell{attrs} style=\"{}\">{content}</Cell>",
            xml(&css.join(";"))
        )
    }
    /// A text body as source markup: inline spans for one plain paragraph, otherwise `<P>`
    /// and nested `<List>` blocks. `base` holds the containing object's text properties.
    fn rich_body(
        &self,
        n: X<'_, '_>,
        body: X<'_, '_>,
        base: &Value,
        part: &SlidePart<'_, '_, '_>,
        problems: &mut Problems,
    ) -> String {
        let SlidePart { links, styles, .. } = *part;
        let props = base;
        // (paragraph properties, list level, inline markup) per paragraph.
        let mut items = Vec::new();
        for p in body.children().filter(|x| x.has_tag_name((A, "p"))) {
            let paragraph = styles.text_properties(n, Some(p), None, problems);
            let level = child(p, A, "pPr")
                .and_then(|p| p.attribute("lvl"))
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0);
            let mut spans = String::new();
            // The marker and indent follow the item's first run.
            let mut first_size = None;
            for run in p.children().filter(|x| x.is_element()) {
                if run.has_tag_name((A, "r")) || run.has_tag_name((A, "fld")) {
                    let value = desc(run, A, "t").and_then(|x| x.text()).unwrap_or("");
                    let mut props = styles.text_properties(n, Some(p), Some(run), problems);
                    first_size = first_size.or_else(|| props.get("size").and_then(Value::as_u64));
                    props.remove("align");
                    let link = child(run, A, "rPr")
                        .and_then(|r| child(r, A, "hlinkClick"))
                        .and_then(|h| h.attribute((R, "id")));
                    let href = link
                        .and_then(|id| links.get(id))
                        .filter(|href| crate::parser::link_target(href).is_ok());
                    if link.is_some() && href.is_none() {
                        problems.insert("hyperlink target is not reconstructed".into());
                    }
                    if let Some(href) = href {
                        // Links take the theme link color and underline.
                        for key in ["color", "underline", "strike"] {
                            props.remove(key);
                        }
                        spans.push_str(&format!(
                            "<A href=\"{}\" style=\"{}\">{}</A>",
                            xml(href),
                            xml(&style_css(&Value::Object(props), true)),
                            xml(value)
                        ));
                    } else {
                        spans.push_str(&format!(
                            "<Span style=\"{}\">{}</Span>",
                            xml(&style_css(&Value::Object(props), true)),
                            xml(value)
                        ));
                    }
                } else if run.has_tag_name((A, "br")) {
                    spans.push_str("<br/>");
                }
            }
            let mut paragraph = paragraph;
            if let Some(size) = first_size {
                paragraph.insert("size".into(), size.into());
            }
            items.push((paragraph, level, spans));
        }
        let bullet = |p: &Map<String, Value>| match p.get("bullet").and_then(Value::as_str) {
            Some("char") => Some(false),
            Some("number") => Some(true),
            _ => None,
        };
        if items.len() == 1 && bullet(&items[0].0).is_none() {
            items.pop().map(|(_, _, spans)| spans).unwrap_or_default()
        } else {
            let mut out = String::new();
            // Open lists, outermost first: whether each is numbered.
            let mut open: Vec<bool> = Vec::new();
            for (paragraph, level, spans) in &items {
                let mut style = Vec::new();
                if paragraph.get("align") != props.get("align")
                    && let Some(align) = paragraph.get("align").and_then(Value::as_str)
                {
                    style.push(format!("text-align:{}", text_align(align)));
                }
                if let Some(before) = paragraph
                    .get("spaceBefore")
                    .and_then(Value::as_u64)
                    .filter(|v| *v > 0)
                {
                    style.push(format!("margin-top:{}px", before as f64 / 75.0));
                }
                let style = if style.is_empty() {
                    String::new()
                } else {
                    format!(" style=\"{}\"", style.join(";"))
                };
                let Some(ordered) = bullet(paragraph) else {
                    for _ in open.drain(..) {
                        out.push_str("</Item></List>");
                    }
                    out.push_str(&format!("<P{style}>{spans}</P>"));
                    continue;
                };
                // A level may only go one deeper than the list it nests in.
                let level = (*level).min(open.len()).min(4);
                while open.len() > level + 1
                    || open.len() == level + 1 && open.last() != Some(&ordered)
                {
                    open.pop();
                    out.push_str("</Item></List>");
                }
                if open.len() == level + 1 {
                    out.push_str("</Item>");
                } else {
                    let start = paragraph
                        .get("bulletStart")
                        .and_then(Value::as_u64)
                        .filter(|v| ordered && *v > 1)
                        .map(|v| format!(" start=\"{v}\""))
                        .unwrap_or_default();
                    let kind = if ordered { "number" } else { "bullet" };
                    out.push_str(&format!("<List type=\"{kind}\"{start}>"));
                    open.push(ordered);
                }
                let size = paragraph
                    .get("size")
                    .and_then(Value::as_u64)
                    .unwrap_or(1800) as f64
                    / 75.0;
                let expected = (level + 1) as f64 * 1.5 * size;
                if let Some(margin) = paragraph.get("marL").and_then(Value::as_i64)
                    && (margin as f64 / 9525.0 - expected).abs() > expected * 0.25
                {
                    problems.insert("list indent is drawn at 1.5em per level".into());
                }
                out.push_str(&format!("<Item{style}>{spans}"));
            }
            for _ in open.drain(..) {
                out.push_str("</Item></List>");
            }
            out
        }
    }
    /// Rebuild one non-group slide object as source XML; unsupported parts become losses.
    fn reconstruct(
        &mut self,
        n: X<'_, '_>,
        part: &SlidePart<'_, '_, '_>,
        target: &Target<'_>,
        problems: &mut Problems,
    ) -> Result<String> {
        let SlidePart {
            page, rels, styles, ..
        } = *part;
        let Target {
            name,
            id: import_id,
            pos,
            css,
            props,
        } = *target;
        let tag = n.tag_name().name();
        Ok(if tag == "pic" {
            if let Some(blip) = desc(n, A, "blip") {
                if let Some(path) = blip.attribute((R, "embed")).and_then(|id| rels.get(id)) {
                    if let Some(bytes) = self.entries.get(path) {
                        let mut bytes = bytes.clone();
                        if let Some(crop) = desc(n, A, "srcRect") {
                            let im = crate::render::decode_image(&bytes)?;
                            let (w, h) = (im.width(), im.height());
                            let frac = |a: &str| {
                                crop.attribute(a)
                                    .and_then(|v| v.parse::<f32>().ok())
                                    .unwrap_or(0.0)
                                    / 100000.0
                            };
                            let l = frac("l");
                            let t = frac("t");
                            let rr = frac("r");
                            let b = frac("b");
                            if [l, t, rr, b].iter().all(|v| *v >= 0.0)
                                && l + rr < 1.0
                                && t + b < 1.0
                            {
                                let cropped = image::imageops::crop_imm(
                                    &im,
                                    (w as f32 * l) as u32,
                                    (h as f32 * t) as u32,
                                    (w as f32 * (1.0 - l - rr)).max(1.0) as u32,
                                    (h as f32 * (1.0 - t - b)).max(1.0) as u32,
                                )
                                .to_image();
                                let mut c = Cursor::new(Vec::new());
                                image::DynamicImage::ImageRgba8(cropped)
                                    .write_to(&mut c, image::ImageFormat::Png)
                                    .map_err(|e| invalid(e.to_string()))?;
                                bytes = c.into_inner();
                            } else {
                                self.loss(page, name, "unsupported image crop");
                            }
                        }
                        let asset = format!("media/{}.png", digest(&bytes));
                        self.assets.insert(asset.clone(), bytes);
                        let mut opacity = 1.0;
                        for effect in blip
                            .children()
                            .filter(|n| n.has_tag_name((A, "alphaModFix")))
                        {
                            if let Some(value) = colors::percentage(effect.attribute("amt")) {
                                opacity = (opacity * value).clamp(0.0, 1.0);
                            } else {
                                problems.insert("invalid image alphaModFix".into());
                            }
                        }
                        // A picture keeps its transform, shadow, border and clip geometry.
                        let kept = css
                            .split(';')
                            .filter(|d| {
                                ["transform:", "box-shadow:", "border-"]
                                    .iter()
                                    .any(|p| d.starts_with(p))
                            })
                            .map(|d| format!(";{d}"))
                            .collect::<String>();
                        let clip = match desc(n, A, "prstGeom") {
                            Some(geom) => match preset_shape(geom) {
                                Some((preset, adjust)) if preset != "rect" => {
                                    format!(" shape=\"{preset}\"{adjust}")
                                }
                                Some(_) => String::new(),
                                None => {
                                    problems.insert(
                                        "picture clip geometry is not reconstructed".into(),
                                    );
                                    String::new()
                                }
                            },
                            None => String::new(),
                        };
                        format!(
                            "<Image id=\"{import_id}\" src=\"{}\"{clip} style=\"{pos};object-fit:fill;opacity:{opacity}{}\"/>",
                            xml(&asset),
                            xml(&kept)
                        )
                    } else {
                        self.loss(page, name, "missing image payload");
                        String::new()
                    }
                } else {
                    self.loss(page, name, "linked image not retrieved");
                    String::new()
                }
            } else {
                String::new()
            }
        } else if let Some(table) = desc(n, A, "tbl") {
            let columns = table
                .descendants()
                .filter(|x| x.has_tag_name((A, "gridCol")))
                .map(|x| x.attribute("w").unwrap_or("1"))
                .collect::<Vec<_>>()
                .join(" ");
            let mut rows = String::new();
            for tr in table.children().filter(|x| x.has_tag_name((A, "tr"))) {
                let height = tr
                    .attribute("h")
                    .and_then(|h| h.parse::<i64>().ok())
                    .filter(|h| *h > 0)
                    .map(|h| format!(" style=\"height:{h}emu\""))
                    .unwrap_or_default();
                rows.push_str(&format!("<Row{height}>"));
                for tc in tr.children().filter(|x| x.has_tag_name((A, "tc"))) {
                    let merged = |k: &str| matches!(tc.attribute(k), Some("1" | "true"));
                    if merged("hMerge") || merged("vMerge") {
                        continue;
                    }
                    rows.push_str(&self.table_cell(n, tc, part, problems));
                }
                rows.push_str("</Row>");
            }
            // Cells carry their own borders; the table adds no default grid.
            format!(
                "<Table id=\"{import_id}\" columns=\"{columns}\" style=\"{pos};{};border-width:0\">{rows}</Table>",
                xml(css)
            )
        } else if let Some(chart) = n.descendants().find(|x| x.tag_name().name() == "chart") {
            if let Some(path) = chart.attribute((R, "id")).and_then(|id| rels.get(id)) {
                if let Some(bytes) = self.entries.get(path) {
                    match chart_source(bytes, import_id, pos, styles, problems) {
                        Ok(xml) => xml,
                        Err(e) => {
                            self.loss(page, name, e.to_string());
                            String::new()
                        }
                    }
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        } else if let Some(body) =
            child(n, P, "txBody").filter(|body| !plain(*body).trim().is_empty())
        {
            let spans = self.rich_body(n, body, props, part, problems);
            let bodypr = child(body, A, "bodyPr");
            let white_space = if bodypr.is_some_and(|p| p.attribute("wrap") == Some("none")) {
                ";white-space:nowrap"
            } else {
                ""
            };
            let anchor = match styles.anchor(n) {
                Some("ctr") => ";vertical-align:middle",
                Some("b") => ";vertical-align:bottom",
                Some("t") | None => "",
                Some(other) => {
                    problems.insert(format!("text anchor {other} is not reconstructed"));
                    ""
                }
            };
            let l = number(bodypr, "lIns", 91440);
            let r = number(bodypr, "rIns", 91440);
            let t = number(bodypr, "tIns", 45720);
            let b = number(bodypr, "bIns", 45720);
            let text = |css: &str| {
                format!(
                    "<Text id=\"{import_id}\" style=\"{pos};{}{white_space}{anchor};padding:{t}emu {r}emu {b}emu {l}emu\">{spans}</Text>",
                    xml(css)
                )
            };
            // A non-rectangular preset draws its fill and outline as a Shape under the text.
            match desc(n, A, "prstGeom").and_then(preset_shape) {
                Some((preset, adjust)) if preset != "rect" => {
                    let boxed = |d: &&str| {
                        ["background", "border-", "box-shadow"]
                            .iter()
                            .any(|p| d.starts_with(p))
                    };
                    let (under, over): (Vec<&str>, Vec<&str>) = css.split(';').partition(boxed);
                    let turn = over
                        .iter()
                        .find(|d| d.starts_with("transform:"))
                        .map(|d| format!(";{d}"))
                        .unwrap_or_default();
                    format!(
                        "<Shape id=\"{import_id}/shape\" shape=\"{preset}\"{adjust} style=\"{pos};{}{}\"/>{}",
                        xml(&under.join(";")),
                        xml(&turn),
                        text(&over.join(";"))
                    )
                }
                _ => text(css),
            }
        } else if let Some(geom) = desc(n, A, "prstGeom") {
            let Some((shape, adjust)) = preset_shape(geom) else {
                self.loss(
                    page,
                    name,
                    format!(
                        "preset shape {} is not reconstructed",
                        geom.attribute("prst").unwrap_or("")
                    ),
                );
                return Ok(String::new());
            };
            let ends = if crate::preset::is_open(&shape) {
                markers(props)
            } else {
                String::new()
            };
            format!(
                "<Shape id=\"{import_id}\" shape=\"{shape}\"{adjust} style=\"{pos};{}{ends}\"/>",
                xml(css)
            )
        } else if let Some(custom) = desc(n, A, "custGeom") {
            if let Some(paths) = custom_paths(custom) {
                // Sub-paths with their own fill/stroke flags become one Path each.
                let single = paths.len() == 1;
                paths
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let mut style = css.to_owned();
                        if !p.fill {
                            style.push_str(";background:transparent");
                        }
                        if !p.stroke {
                            style.push_str(";border-width:0");
                        }
                        format!(
                            "<Path id=\"{import_id}{}\" d=\"{}\" viewBox=\"{} {}\" style=\"{pos};{}{}\"/>",
                            if single { String::new() } else { format!("/{}", i + 1) },
                            xml(&p.data),
                            p.size[0],
                            p.size[1],
                            xml(&style),
                            if single { markers(props) } else { String::new() }
                        )
                    })
                    .collect()
            } else {
                self.loss(page, name, "custom path needs unsupported guide evaluation");
                String::new()
            }
        } else if child(n, P, "txBody").is_some() {
            // Empty text placeholders without shape geometry are retained,
            // but paragraph markers and insets do not invent visible text.
            // Shapes/paths with an empty txBody take their geometry branch.
            format!("<Text id=\"{import_id}\" style=\"{pos};{}\"/>", xml(css))
        } else {
            self.loss(page, name, "object not reconstructable");
            String::new()
        })
    }
}

/// Translate resolved OOXML properties to source CSS. Span-only properties (baseline
/// shift, highlight) are written only for spans: on a Text they mean something else.
fn style_css(p: &Value, span: bool) -> String {
    let mut css = Vec::new();
    for (k, prop) in [
        ("family", "font-family"),
        ("color", "color"),
        ("fill", "background-color"),
        ("stroke", "border-color"),
    ] {
        if let Some(s) = p[k].as_str()
            && !s.is_empty()
        {
            css.push(format!(
                "{prop}:{}{s}",
                if k == "family" { "" } else { "#" }
            ));
        }
    }
    if let Some(n) = p["size"].as_u64() {
        css.push(format!("font-size:{}pt", n as f64 / 100.0));
    }
    if let Some(n) = p["lineHeight"].as_u64()
        && n > 0
    {
        css.push(format!("line-height:{}pt", n as f64 / 100.0));
    }
    if let Some(n) = p["lineHeightPct"].as_f64() {
        css.push(format!("line-height:{n}"));
    }
    if p["weight"].as_str() == Some("1") {
        css.push("font-weight:700".into());
    } else if p["weight"].as_str() == Some("0") {
        css.push("font-weight:400".into());
    }
    if p["italic"].as_str() == Some("1") {
        css.push("font-style:italic".into());
    } else if p["italic"].as_str() == Some("0") {
        css.push("font-style:normal".into());
    }
    if let Some(n) = p["strokeWidth"].as_u64() {
        css.push(format!("border-width:{n}emu"));
    }
    if let Some(a) = p["align"].as_str() {
        let a = match a {
            "ctr" => "center",
            "r" => "right",
            "just" => "justify",
            _ => "left",
        };
        css.push(format!("text-align:{a}"));
    }
    if let Some(gradient) = p["gradient"].as_str() {
        css.retain(|d| !d.starts_with("background-color:"));
        css.push(format!("background:{gradient}"));
    }
    if let Some(shadow) = p["shadow"].as_str() {
        css.push(format!("box-shadow:{shadow}"));
    }
    if let Some(dash) = p["dash"].as_str() {
        css.push(format!("border-style:{dash}"));
    }
    if let Some(cap) = p["lineCap"].as_str().filter(|c| *c != "butt") {
        css.push(format!("stroke-linecap:{cap}"));
    }
    if let Some(join) = p["lineJoin"].as_str().filter(|j| *j != "miter") {
        css.push(format!("stroke-linejoin:{join}"));
    }
    let underline = p["underline"].as_bool();
    let strike = p["strike"].as_bool();
    if underline.is_some() || strike.is_some() {
        let lines = [
            (underline == Some(true)).then_some("underline"),
            (strike == Some(true)).then_some("line-through"),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        css.push(format!(
            "text-decoration:{}",
            if lines.is_empty() {
                "none".into()
            } else {
                lines.join(" ")
            }
        ));
    }
    if let Some(spacing) = p["letterSpacing"].as_i64() {
        css.push(format!("letter-spacing:{}px", spacing as f64 / 75.0));
    }
    if span {
        if let Some(shift) = p["baseline"].as_str() {
            css.push(format!("vertical-align:{shift}"));
        }
        if let Some(color) = p["highlight"].as_str() {
            css.push(format!("background-color:#{color}"));
        }
    }
    css.join(";")
}
fn payload(
    n: X<'_, '_>,
    rels: &BTreeMap<String, String>,
    entries: &BTreeMap<String, Vec<u8>>,
) -> Result<String> {
    if let Some(blip) = desc(n, A, "blip")
        && let Some(data) = blip
            .attribute((R, "embed"))
            .and_then(|id| rels.get(id))
            .and_then(|p| entries.get(p))
    {
        return Ok(digest(data));
    }
    if let Some(c) = n.descendants().find(|x| x.tag_name().name() == "chart")
        && let Some(data) = c
            .attribute((R, "id"))
            .and_then(|id| rels.get(id))
            .and_then(|p| entries.get(p))
    {
        return semantic_hash(data);
    }
    if let Some(table) = desc(n, A, "tbl") {
        return Ok(digest(&serde_json::to_vec(&canonical(table))?));
    }
    if let Some(path) = desc(n, A, "custGeom") {
        return Ok(digest(&serde_json::to_vec(&canonical(path))?));
    }
    Ok(String::new())
}
/// One custom-geometry sub-path as SVG path data in its own coordinate box.
struct CustomPath {
    data: String,
    size: [f32; 2],
    fill: bool,
    stroke: bool,
}
/// Every sub-path of a custGeom, or `None` when one needs guide evaluation.
fn custom_paths(n: X<'_, '_>) -> Option<Vec<CustomPath>> {
    let mut out = Vec::new();
    for p in n.descendants().filter(|x| x.has_tag_name((A, "path"))) {
        let w = p.attribute("w")?.parse::<f32>().ok()?;
        let h = p.attribute("h")?.parse::<f32>().ok()?;
        let mut data = String::new();
        let mut pen = (0.0f64, 0.0f64);
        for c in p.children().filter(|n| n.is_element()) {
            let points = c
                .children()
                .filter(|x| x.has_tag_name((A, "pt")))
                .map(|p| {
                    Some((
                        p.attribute("x")?.parse::<f64>().ok()?,
                        p.attribute("y")?.parse::<f64>().ok()?,
                    ))
                })
                .collect::<Option<Vec<_>>>()?;
            let (tag, need) = match c.tag_name().name() {
                "moveTo" => ("M", 1),
                "lnTo" => ("L", 1),
                "quadBezTo" => ("Q", 2),
                "cubicBezTo" => ("C", 3),
                "close" => ("Z", 0),
                "arcTo" => {
                    let num = |k: &str| c.attribute(k)?.parse::<f64>().ok();
                    let (rx, ry, st, sw) = (num("wR")?, num("hR")?, num("stAng")?, num("swAng")?);
                    pen = crate::preset::arc(pen, rx, ry, st, sw, &mut |cmd| {
                        if let crate::render::Command::C(a, b, c, d, e, f) = cmd {
                            data.push_str(&format!("C {a} {b} {c} {d} {e} {f} "));
                        }
                    });
                    continue;
                }
                _ => return None,
            };
            if points.len() != need {
                return None;
            }
            data.push_str(tag);
            for (x, y) in &points {
                data.push_str(&format!(" {x} {y}"));
            }
            data.push(' ');
            pen = points.last().copied().unwrap_or(pen);
        }
        out.push(CustomPath {
            data,
            size: [w.max(1.0), h.max(1.0)],
            fill: p.attribute("fill") != Some("none"),
            stroke: !matches!(p.attribute("stroke"), Some("0" | "false")),
        });
    }
    (!out.is_empty()).then_some(out)
}
/// A native chart as a `<Chart>` with every type and option the language has; anything
/// else is reported.
fn chart_source(
    bytes: &[u8],
    id: &str,
    pos: &str,
    styles: &Styles<'_, '_>,
    problems: &mut Problems,
) -> Result<String> {
    let doc = parse_xml(bytes)?;
    let root = doc.root_element();
    fn named<'a, 'i>(n: X<'a, 'i>, name: &str) -> Option<X<'a, 'i>> {
        n.children().find(|c| c.tag_name().name() == name)
    }
    let val = |n: X<'_, '_>, name: &str| {
        n.children()
            .find(|c| c.tag_name().name() == name)
            .map(|c| c.attribute("val").unwrap_or("1").to_owned())
    };
    let on = |n: X<'_, '_>, name: &str| val(n, name).is_some_and(|v| v == "1" || v == "true");
    let chart = named(root, "chart").ok_or_else(|| invalid("chart part has no chart"))?;
    let plot = named(chart, "plotArea").ok_or_else(|| invalid("chart has no plot area"))?;
    let mut groups = Vec::new();
    for g in plot.children().filter(|g| g.is_element()) {
        let kind = match g.tag_name().name() {
            "barChart" => ChartType::Bar,
            "lineChart" => ChartType::Line,
            "areaChart" => ChartType::Area,
            "pieChart" => ChartType::Pie,
            "doughnutChart" => ChartType::Doughnut,
            "scatterChart" => ChartType::Scatter,
            other if other.ends_with("Chart") => {
                return Err(invalid(format!("{other} charts are not reconstructed")));
            }
            _ => continue,
        };
        groups.push((kind, g));
    }
    let &(kind, base) = groups.first().ok_or_else(|| invalid("chart has no data"))?;
    if groups.len() > 1
        && groups
            .iter()
            .any(|(k, _)| !matches!(k, ChartType::Bar | ChartType::Line | ChartType::Area))
    {
        return Err(invalid(
            "pie, doughnut and scatter combinations are not reconstructed",
        ));
    }
    let axes_of = |g: X<'_, '_>| {
        g.children()
            .filter(|c| c.tag_name().name() == "axId")
            .filter_map(|c| c.attribute("val").map(str::to_owned))
            .collect::<Vec<_>>()
    };
    if groups.iter().any(|(_, g)| axes_of(*g) != axes_of(base)) {
        problems.insert("chart secondary axis is drawn on the primary axis".into());
    }
    let accent = |i: usize, problems: &mut Problems| {
        styles
            .theme
            .element("clrScheme")
            .and_then(|s| child(s, A, &format!("accent{}", i % 6 + 1)))
            .and_then(|a| styles.theme.color_child(a, None, problems))
            .unwrap_or(crate::chart::PALETTE[i % crate::chart::PALETTE.len()])
    };
    // Cached points by index; a blank point becomes 0.
    let points = |n: Option<X<'_, '_>>, problems: &mut Problems| -> Vec<String> {
        let Some(n) = n else { return Vec::new() };
        let count = n
            .descendants()
            .find(|c| c.tag_name().name() == "ptCount")
            .and_then(|c| c.attribute("val")?.parse::<usize>().ok())
            .unwrap_or(0)
            .min(100);
        let mut out = vec![None; count];
        for pt in n.descendants().filter(|c| c.tag_name().name() == "pt") {
            if let Some(i) = pt.attribute("idx").and_then(|v| v.parse::<usize>().ok())
                && i < count
            {
                out[i] = named(pt, "v").and_then(|v| v.text()).map(str::to_owned);
            }
        }
        if out.iter().any(Option::is_none) {
            problems.insert("blank chart points become 0".into());
        }
        out.into_iter().map(|v| v.unwrap_or("0".into())).collect()
    };
    let mut categories = Vec::new();
    let mut series = String::new();
    let mut names = Vec::new();
    let (mut smooth, mut markers) = (false, false);
    for &(k, g) in &groups {
        for s in g.children().filter(|c| c.tag_name().name() == "ser") {
            let (cat, values) = if k == ChartType::Scatter {
                (named(s, "xVal"), named(s, "yVal"))
            } else {
                (named(s, "cat"), named(s, "val"))
            };
            if categories.is_empty() {
                categories = points(cat, problems);
            }
            let values = points(values, problems);
            let name = named(s, "tx")
                .and_then(|t| t.descendants().find(|c| c.tag_name().name() == "v"))
                .and_then(|v| v.text())
                .unwrap_or("Series")
                .to_owned();
            let paint = |n: X<'_, '_>, problems: &mut Problems| {
                named(n, "spPr").and_then(|sp| {
                    let target = if matches!(k, ChartType::Line) {
                        child(sp, A, "ln")?
                    } else {
                        sp
                    };
                    styles.theme.fill(target, None, problems)
                })
            };
            let colors = if k.is_round() {
                let mut colors = (0..values.len())
                    .map(|j| accent(j, problems))
                    .collect::<Vec<_>>();
                for pt in s.children().filter(|c| c.tag_name().name() == "dPt") {
                    if let Some(j) = val(pt, "idx").and_then(|v| v.parse::<usize>().ok())
                        && j < colors.len()
                        && let Some(c) = paint(pt, problems)
                    {
                        colors[j] = c;
                    }
                }
                format!(
                    " colors=\"{}\"",
                    colors
                        .iter()
                        .map(|c| format!("#{}", color_hex(*c)))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            } else {
                let c = paint(s, problems).unwrap_or_else(|| accent(names.len(), problems));
                format!(" color=\"#{}\"", color_hex(c))
            };
            if k == ChartType::Line {
                smooth |= on(s, "smooth");
                let symbol = named(s, "marker").and_then(|m| val(m, "symbol"));
                markers |= match symbol.as_deref() {
                    Some("none") => false,
                    Some(_) => true,
                    None => on(g, "marker"),
                };
            }
            if k == ChartType::Scatter
                && named(s, "spPr")
                    .and_then(|sp| child(sp, A, "ln"))
                    .is_none_or(|ln| child(ln, A, "noFill").is_none())
                && !matches!(val(g, "scatterStyle").as_deref(), Some("marker" | "none"))
            {
                problems.insert("scatter chart lines are not reconstructed".into());
            }
            let own = if k != kind {
                format!(
                    " type=\"{}\"",
                    serde_json::to_value(k)?.as_str().unwrap_or("bar")
                )
            } else {
                String::new()
            };
            series.push_str(&format!(
                "<Series name=\"{}\"{own}{colors}>{}</Series>",
                xml(&name),
                xml(&values.join(" "))
            ));
            names.push(name);
        }
    }
    let mut attrs = format!(
        " type=\"{}\"",
        serde_json::to_value(kind)?.as_str().unwrap_or("bar")
    );
    let mut add = |k: &str, v: &str| attrs.push_str(&format!(" {k}=\"{}\"", xml(v)));
    if kind == ChartType::Bar && val(base, "barDir").as_deref() == Some("bar") {
        add("orient", "horizontal");
    }
    let stack = match val(base, "grouping").as_deref() {
        Some("stacked") => "normal",
        Some("percentStacked") => "percent",
        _ => "none",
    };
    if stack != "none" {
        add("stack", stack);
    }
    let round = kind.is_round();
    let legend = named(chart, "legend").map(|l| match val(l, "legendPos").as_deref() {
        Some("b") => "bottom",
        Some("t") => "top",
        Some("l") => "left",
        _ => "right",
    });
    if legend != (round || names.len() > 1).then_some("bottom") {
        add("legend", legend.unwrap_or("none"));
    }
    let labels = plot
        .descendants()
        .filter(|n| n.tag_name().name() == "dLbls" && !on(*n, "delete"))
        .find_map(|d| {
            if on(d, "showPercent") {
                Some("percent")
            } else if on(d, "showCatName") {
                Some("category")
            } else if on(d, "showVal") {
                Some("value")
            } else {
                None
            }
        });
    let area = groups.iter().any(|(k, _)| *k == ChartType::Area);
    match labels {
        Some(l) if area || !round && l != "value" => {
            problems.insert(format!(
                "chart data labels ({l}) are not reconstructed here"
            ));
        }
        Some(l) => add("data-labels", l),
        None => {}
    }
    // The value axis: the scatter y axis, or the one beside the categories.
    let value_axis = plot.children().find(|a| {
        a.tag_name().name() == "valAx"
            && (kind != ChartType::Scatter
                || matches!(val(*a, "axPos").as_deref(), Some("l" | "r")))
    });
    let mut format = None;
    if let Some(ax) = value_axis {
        if stack != "percent" {
            let scaling = named(ax, "scaling");
            for (k, v) in [
                ("min", scaling.and_then(|s| val(s, "min"))),
                ("max", scaling.and_then(|s| val(s, "max"))),
                ("step", val(ax, "majorUnit")),
            ] {
                if let Some(v) = v {
                    add(k, &v);
                }
            }
            format = named(ax, "numFmt")
                .filter(|f| f.attribute("sourceLinked") != Some("1"))
                .and_then(|f| f.attribute("formatCode"));
        }
        if named(ax, "majorGridlines").is_none() {
            add("grid", "false");
        }
    }
    let format = format.or_else(|| {
        plot.descendants()
            .find(|n| n.tag_name().name() == "dLbls")
            .and_then(|d| named(d, "numFmt"))
            .and_then(|f| f.attribute("formatCode"))
            .filter(|_| labels == Some("value"))
    });
    if let Some(f) = format.filter(|f| !f.eq_ignore_ascii_case("general")) {
        if crate::chart::format_value(0.0, f).is_ok() {
            add("format", f);
        } else {
            problems.insert(format!("chart number format {f} is not reconstructed"));
        }
    }
    if let Some(t) = named(chart, "title") {
        let text = t
            .descendants()
            .filter(|n| n.tag_name().name() == "t")
            .filter_map(|n| n.text())
            .collect::<String>();
        // A title without its own text shows the only series' name.
        let text = match names.as_slice() {
            [only] if text.is_empty() => only.clone(),
            _ => text,
        };
        if !text.trim().is_empty() {
            add("title", &text);
        }
    }
    if smooth {
        add("smooth", "true");
    }
    if markers {
        add("markers", "true");
    }
    if categories.iter().any(|c| c.contains('|')) {
        problems.insert("'|' in chart labels becomes '/'".into());
    }
    let mut css = styles.standalone_text(named(root, "txPr").unwrap_or(root), problems);
    if let Some(sp) = named(root, "spPr") {
        if let Some(c) = styles.theme.fill(sp, None, problems).filter(|c| c.3 > 0) {
            css.insert("fill".into(), json!(color_hex(c)));
        }
        if let Some(ln) = child(sp, A, "ln")
            && let Some(c) = styles.theme.fill(ln, None, problems).filter(|c| c.3 > 0)
        {
            css.insert("stroke".into(), json!(color_hex(c)));
            css.insert(
                "strokeWidth".into(),
                json!(
                    ln.attribute("w")
                        .and_then(|w| w.parse::<u64>().ok())
                        .unwrap_or(9525)
                ),
            );
        }
    }
    Ok(format!(
        "<Chart id=\"{id}\"{attrs} style=\"{pos};{}\"><Labels>{}</Labels>{series}</Chart>",
        xml(&style_css(&Value::Object(css), false)),
        xml(&categories
            .iter()
            .map(|c| c.replace('|', "/"))
            .collect::<Vec<_>>()
            .join("|"))
    ))
}

pub fn snapshot_json(snapshot: &Snapshot) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec_pretty(snapshot)?)
}
fn text_align(algn: &str) -> &'static str {
    match algn {
        "ctr" => "center",
        "r" => "right",
        "just" => "justify",
        _ => "left",
    }
}
/// Line-end markers; only lines and paths can carry them.
fn markers(p: &Value) -> String {
    [("markerStart", "marker-start"), ("markerEnd", "marker-end")]
        .iter()
        .filter_map(|(k, css)| p[*k].as_str().map(|m| format!(";{css}:{m}")))
        .collect()
}
/// The `shape` name and `adjust` attribute for a preset geometry; `roundRect` becomes a rect
/// with border-radius. Unknown presets or non-constant adjust formulas give `None`.
fn preset_shape(geom: X<'_, '_>) -> Option<(String, String)> {
    let prst = geom.attribute("prst").unwrap_or("rect");
    if matches!(prst, "rect" | "roundRect") {
        return Some(("rect".into(), String::new()));
    }
    if !crate::preset::exists(prst) {
        return None;
    }
    let given = geom
        .descendants()
        .filter(|g| g.has_tag_name((A, "gd")))
        .map(|g| {
            let value = g
                .attribute("fmla")?
                .strip_prefix("val ")?
                .trim()
                .parse::<f64>()
                .ok()?;
            Some((g.attribute("name")?, value))
        })
        .collect::<Option<BTreeMap<_, _>>>()?;
    let defaults = crate::preset::catalog()
        .into_iter()
        .find(|(name, _)| *name == prst)
        .map(|(_, adjust)| adjust)
        .unwrap_or_default();
    let adjust = if given.is_empty() {
        String::new()
    } else {
        let values = defaults
            .iter()
            .map(|(name, default)| given.get(name).copied().unwrap_or(*default).to_string())
            .collect::<Vec<_>>();
        format!(" adjust=\"{}\"", values.join(" "))
    };
    Some((prst.into(), adjust))
}
