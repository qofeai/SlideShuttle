//! Bounded package inspection and explicitly lossy reconstruction into our own format.
mod colors;
mod styles;
use crate::{
    model::*,
    pptx::{A, P, R},
};
use colors::Problems;
use roxmltree::{Document, Node as X};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
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
    pub xml: String,
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
        if !["sldMasterIdLst", "sldIdLst", "sldSz", "notesSz"].contains(&tag) {
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
        for feature in ["timing", "transition", "extLst"] {
            if let Some(n) = child(root, P, feature) {
                inventory.insert(
                    format!("{path}#{feature}"),
                    digest(&serde_json::to_vec(&canonical(n))?),
                );
                reader.losses.push(Loss {
                    page: page.clone(),
                    object: feature.into(),
                    reason: format!(
                        "{feature} is retained in the inventory but is not reconstructed"
                    ),
                });
            }
        }
        let mut problems = Problems::new();
        let bg = styles.background(&mut problems);
        for reason in problems {
            reader.losses.push(Loss {
                page: page.clone(),
                object: "background".into(),
                reason,
            });
        }
        slides.push(format!(
            "<Slide id=\"{page}\" background=\"{}\">{}</Slide>",
            xml(&bg),
            reader.nodes.join("")
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
            || name.contains("/notesSlides")
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
                reader.losses.push(Loss {
                    page: "package".into(),
                    object: name.clone(),
                    reason: format!(
                        "external relationship not fetched: {}",
                        rel.attribute("Type").unwrap_or("unknown")
                    ),
                });
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
    let xml = format!(
        "<Deck version=\"1\" title=\"Imported presentation\" width=\"{width}emu\" height=\"{height}emu\" font-family=\"Noto Sans CJK SC\">{asset_xml}{}</Deck>",
        slides.join("")
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
        xml,
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
            if xf.is_some_and(|x| {
                x.attribute("rot").is_some_and(|v| v != "0")
                    || x.attribute("flipH") == Some("1")
                    || x.attribute("flipV") == Some("1")
            }) {
                self.loss(page, &name, "rotation/flip is not reconstructed");
            }
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
            for feature in ["gradFill", "effectLst", "effectDag", "blip", "prstDash"] {
                let unsupported = n.descendants().any(|x| {
                    x.has_tag_name((A, feature))
                        && match feature {
                            "gradFill" | "effectDag" => true,
                            "effectLst" => x.children().any(|c| c.is_element()),
                            "blip" => x
                                .children()
                                .any(|c| c.is_element() && !c.has_tag_name((A, "alphaModFix"))),
                            "prstDash" => x.attribute("val") != Some("solid"),
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
            let mut css = style_css(&props);
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
                        format!(
                            "<Image id=\"{import_id}\" src=\"{}\" style=\"{pos};object-fit:fill;opacity:{opacity}\"/>",
                            xml(&asset)
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
                rows.push_str("<Row>");
                for tc in tr.children().filter(|x| x.has_tag_name((A, "tc"))) {
                    rows.push_str(&format!("<Cell>{}</Cell>", xml(&plain(tc))));
                    if tc.attribute("gridSpan").is_some() || tc.attribute("rowSpan").is_some() {
                        self.loss(page, name, "merged table cells are not reconstructed");
                    }
                }
                rows.push_str("</Row>");
            }
            format!(
                "<Table id=\"{import_id}\" columns=\"{columns}\" style=\"{pos};{}\">{rows}</Table>",
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
            let mut spans = String::new();
            for (pi, p) in body
                .children()
                .filter(|x| x.has_tag_name((A, "p")))
                .enumerate()
            {
                if pi > 0 {
                    spans.push_str("<br/>");
                }
                let paragraph = styles.text_properties(n, Some(p), None, problems);
                if pi > 0 && paragraph.get("align") != props.get("align") {
                    problems.insert(
                        "mixed paragraph alignments in one text object are not reconstructed"
                            .into(),
                    );
                }
                for run in p.children().filter(|x| x.is_element()) {
                    if run.has_tag_name((A, "r")) || run.has_tag_name((A, "fld")) {
                        let value = desc(run, A, "t").and_then(|x| x.text()).unwrap_or("");
                        let mut props = styles.text_properties(n, Some(p), Some(run), problems);
                        props.remove("align");
                        let style = style_css(&Value::Object(props));
                        spans.push_str(&format!(
                            "<Span style=\"{}\">{}</Span>",
                            xml(&style),
                            xml(value)
                        ));
                    } else if run.has_tag_name((A, "br")) {
                        spans.push_str("<br/>");
                    }
                }
            }
            let bodypr = child(body, A, "bodyPr");
            let white_space = if bodypr.is_some_and(|p| p.attribute("wrap") == Some("none")) {
                ";white-space:nowrap"
            } else {
                ""
            };
            let l = number(bodypr, "lIns", 91440);
            let r = number(bodypr, "rIns", 91440);
            let t = number(bodypr, "tIns", 45720);
            let b = number(bodypr, "bIns", 45720);
            format!(
                "<Text id=\"{import_id}\" style=\"{pos};{}{white_space};padding:{t}emu {r}emu {b}emu {l}emu\">{spans}</Text>",
                xml(css)
            )
        } else if let Some(geom) = desc(n, A, "prstGeom") {
            let shape = match geom.attribute("prst").unwrap_or("rect") {
                "rect" | "roundRect" => "rect",
                "ellipse" => "ellipse",
                "line" => "line",
                other => {
                    self.loss(
                        page,
                        name,
                        format!("preset shape {other} approximated as rectangle"),
                    );
                    "rect"
                }
            };
            format!(
                "<Shape id=\"{import_id}\" shape=\"{shape}\" style=\"{pos};{}\"/>",
                xml(css)
            )
        } else if let Some(custom) = desc(n, A, "custGeom") {
            if let Some((data, vb)) = custom_path(custom) {
                format!(
                    "<Path id=\"{import_id}\" d=\"{}\" viewBox=\"{} {}\" style=\"{pos};{}\"/>",
                    xml(&data),
                    vb[0],
                    vb[1],
                    xml(css)
                )
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

fn style_css(p: &Value) -> String {
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
fn custom_path(n: X<'_, '_>) -> Option<(String, [f32; 2])> {
    let paths = n
        .descendants()
        .filter(|x| x.has_tag_name((A, "path")))
        .collect::<Vec<_>>();
    if paths.len() != 1 {
        return None;
    }
    let p = paths[0];
    let w = p.attribute("w")?.parse::<f32>().ok()?;
    let h = p.attribute("h")?.parse::<f32>().ok()?;
    let mut data = String::new();
    for c in p.children().filter(|n| n.is_element()) {
        let points = c
            .children()
            .filter(|x| x.has_tag_name((A, "pt")))
            .map(|p| {
                Some((
                    p.attribute("x")?.parse::<f32>().ok()?,
                    p.attribute("y")?.parse::<f32>().ok()?,
                ))
            })
            .collect::<Option<Vec<_>>>()?;
        match c.tag_name().name() {
            "moveTo" | "lnTo" => {
                let (x, y) = *points.first()?;
                data.push_str(&format!(
                    "{} {x} {y} ",
                    if c.tag_name().name() == "moveTo" {
                        "M"
                    } else {
                        "L"
                    }
                ));
            }
            "cubicBezTo" => {
                if points.len() != 3 {
                    return None;
                }
                data.push_str("C ");
                for (x, y) in points {
                    data.push_str(&format!("{x} {y} "));
                }
            }
            "close" => data.push_str("Z "),
            _ => return None,
        }
    }
    Some((data, [w, h]))
}
fn chart_source(
    bytes: &[u8],
    id: &str,
    pos: &str,
    styles: &Styles<'_, '_>,
    problems: &mut Problems,
) -> Result<String> {
    let doc = parse_xml(bytes)?;
    let root = doc.root_element();
    let kind = if root
        .descendants()
        .any(|n| n.tag_name().name() == "barChart")
    {
        "bar"
    } else if root
        .descendants()
        .any(|n| n.tag_name().name() == "lineChart")
    {
        "line"
    } else {
        return Err(invalid("unsupported chart type on import"));
    };
    let mut labels = Vec::new();
    let mut series = String::new();
    for s in root.descendants().filter(|n| n.tag_name().name() == "ser") {
        let field = |name: &str| s.children().find(|n| n.tag_name().name() == name);
        let values = |n: X<'_, '_>| {
            n.descendants()
                .filter(|n| n.tag_name().name() == "v")
                .filter_map(|n| n.text())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        if labels.is_empty()
            && let Some(n) = field("cat")
        {
            labels = values(n);
        }
        let name = field("tx")
            .map(values)
            .and_then(|v| v.first().cloned())
            .unwrap_or("Series".into());
        let nums = field("val").map(values).unwrap_or_default();
        let color = s
            .children()
            .find(|n| n.tag_name().name() == "spPr")
            .and_then(|n| {
                styles.theme.fill(
                    if kind == "line" {
                        child(n, A, "ln").unwrap_or(n)
                    } else {
                        n
                    },
                    None,
                    problems,
                )
            })
            .map(colors::color_hex)
            .unwrap_or("2563EB".into());
        series.push_str(&format!(
            "<Series name=\"{}\" color=\"#{color}\">{}</Series>",
            xml(&name),
            xml(&nums.join(" "))
        ));
    }
    Ok(format!(
        "<Chart id=\"{id}\" type=\"{kind}\" style=\"{pos};{}\"><Labels>{}</Labels>{series}</Chart>",
        xml(&style_css(&Value::Object(
            styles.standalone_text(root, problems)
        ))),
        xml(&labels.join("|"))
    ))
}

pub fn snapshot_json(snapshot: &Snapshot) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec_pretty(snapshot)?)
}
