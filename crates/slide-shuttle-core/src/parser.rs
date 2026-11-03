use crate::model::*;
use roxmltree::Node as X;
use std::collections::{BTreeMap, BTreeSet};

const MAX_FILE: usize = 4 * 1024 * 1024;
const MAX_PROJECT: usize = 64 * 1024 * 1024;
/// Per XML file and per expanded slide.
const MAX_NODES: usize = 20_000;
const PROPS: &[&str] = &[
    "display",
    "width",
    "height",
    "min-width",
    "max-width",
    "min-height",
    "max-height",
    "padding",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "margin",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "gap",
    "row-gap",
    "column-gap",
    "flex-direction",
    "flex-grow",
    "flex-shrink",
    "flex-basis",
    "align-items",
    "align-self",
    "justify-content",
    "grid-template-columns",
    "grid-template-rows",
    "grid-column",
    "grid-row",
    "position",
    "left",
    "top",
    "right",
    "bottom",
    "font-family",
    "font-size",
    "font-weight",
    "font-style",
    "line-height",
    "color",
    "text-align",
    "background",
    "background-color",
    "border",
    "border-width",
    "border-color",
    "border-radius",
    "opacity",
    "chart-colors",
    "object-fit",
    "object-position",
    "box-sizing",
    "white-space",
    "letter-spacing",
    "text-decoration",
    "vertical-align",
    "border-style",
    "box-shadow",
    "transform",
    "marker-start",
    "marker-end",
    "stroke-linecap",
    "stroke-linejoin",
    "border-top",
    "border-right",
    "border-bottom",
    "border-left",
    "border-top-width",
    "border-right-width",
    "border-bottom-width",
    "border-left-width",
    "border-top-style",
    "border-right-style",
    "border-bottom-style",
    "border-left-style",
    "border-top-color",
    "border-right-color",
    "border-bottom-color",
    "border-left-color",
];
/// Properties a table Cell may set; a Row may also set `height`.
const CELL_PROPS: &[&str] = &[
    "font-family",
    "font-size",
    "font-weight",
    "font-style",
    "line-height",
    "color",
    "text-align",
    "letter-spacing",
    "text-decoration",
    "background",
    "vertical-align",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "border-width",
    "border-style",
    "border-color",
    "border-top-width",
    "border-right-width",
    "border-bottom-width",
    "border-left-width",
    "border-top-style",
    "border-right-style",
    "border-bottom-style",
    "border-left-style",
    "border-top-color",
    "border-right-color",
    "border-bottom-color",
    "border-left-color",
];
/// Properties a `Span` or `A` may set; `background` (from `background-color`) is a highlight.
const SPAN_PROPS: &[&str] = &[
    "font-family",
    "font-size",
    "font-weight",
    "font-style",
    "line-height",
    "color",
    "letter-spacing",
    "text-decoration",
    "vertical-align",
    "background",
];
const PARAGRAPH_PROPS: &[&str] = &["text-align", "margin-top"];

/// Parse a self-contained source string; `<Library src>` and `<Slide src>` need [`parse_project`].
pub fn parse(source: &str) -> Result<Deck> {
    parse_project("", source, |path| {
        Err(invalid(format!(
            "{path}: file references need a project loader"
        )))
    })
}

/// Parse a project held in memory, keyed by project-relative path.
pub fn parse_files(entry: &str, files: &BTreeMap<String, String>) -> Result<Deck> {
    let source = files
        .get(entry)
        .ok_or_else(|| invalid(format!("missing entry file {entry}")))?;
    parse_project(entry, source, |path| {
        files
            .get(path)
            .cloned()
            .ok_or_else(|| invalid(format!("missing {path}")))
    })
}
/// Parse an entry file and the libraries and pages it references. `read` receives paths
/// relative to the entry's directory, already checked to stay inside it.
pub fn parse_project(
    entry: &str,
    source: &str,
    mut read: impl FnMut(&str) -> Result<String>,
) -> Result<Deck> {
    let entry_doc = document(entry, source)?;
    let root = entry_doc.root_element();
    let located = |n: X<'_, '_>, e: Error| at(&position(entry, n), e);
    if root.tag_name().name() != "Deck" {
        return Err(located(root, invalid("root must be <Deck>")));
    }
    attrs(root, &["title", "width", "height", "font-family"]).map_err(|e| located(root, e))?;
    let width = length_px(root.attribute("width").unwrap_or("1280px"))?;
    let height = length_px(root.attribute("height").unwrap_or("720px"))?;
    if width <= 0.0 || height <= 0.0 || width > 16384.0 || height > 16384.0 {
        return Err(located(root, invalid("page size must be in (0, 16384]px")));
    }
    let mut errors = Errors::default();
    // Read every referenced file first, so all documents share one lifetime.
    let mut texts = BTreeMap::new();
    let mut total = source.len();
    for n in references(root) {
        let text = reference(n).and_then(|path| {
            if let std::collections::btree_map::Entry::Vacant(slot) = texts.entry(path) {
                let text = read(slot.key())?;
                total += text.len();
                if total > MAX_PROJECT {
                    return Err(invalid("project sources exceed 64 MiB"));
                }
                slot.insert(text);
            }
            Ok(())
        });
        errors.keep(position(entry, n), text);
    }
    let mut docs = BTreeMap::new();
    for (path, text) in &texts {
        // document() errors already carry their file and position.
        if let Some(doc) = errors.keep(position(entry, root), document(path, text)) {
            docs.insert(path.as_str(), doc);
        }
    }
    let mut cx = Context {
        file: entry.into(),
        ..Context::default()
    };
    let mut styles = Vec::new();
    for n in root.children().filter(|n| n.is_element()) {
        let defined = cx.define(n, true, &mut styles);
        errors.keep(cx.source(n), defined);
    }
    for n in references(root).filter(|n| n.tag_name().name() == "Library") {
        let Some(doc) = reference(n)
            .ok()
            .and_then(|path| docs.get_key_value(path.as_str()))
        else {
            continue;
        };
        cx.file = (*doc.0).into();
        let library = doc.1.root_element();
        let checked = if library.tag_name().name() == "Library" {
            attrs(library, &[])
        } else {
            Err(invalid("a library file's root must be <Library>"))
        };
        if errors.keep(cx.source(library), checked).is_some() {
            for n in library.children().filter(|n| n.is_element()) {
                let defined = cx.define(n, false, &mut styles);
                errors.keep(cx.source(n), defined);
            }
        }
    }
    for (file, n) in styles {
        cx.file = file;
        let checked = cx.css(n.text().unwrap_or(""));
        errors.keep(cx.source(n), checked);
    }
    let mut slides = Vec::new();
    for n in root
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "Slide")
    {
        cx.file = entry.into();
        let page = if n.attribute("src").is_some() {
            let Some(doc) = reference(n)
                .ok()
                .and_then(|path| docs.get_key_value(path.as_str()))
            else {
                continue;
            };
            cx.file = (*doc.0).into();
            doc.1.root_element()
        } else {
            n
        };
        let slide = if page.tag_name().name() == "Slide" {
            cx.slide(page)
        } else {
            Err(invalid("a page file's root must be <Slide>"))
        };
        slides.extend(errors.keep(cx.source(page), slide));
    }
    errors.finish()?;
    if slides.is_empty() || slides.len() > 300 {
        return Err(invalid("Deck requires 1–300 slides"));
    }
    Ok(Deck {
        title: root.attribute("title").unwrap_or("Untitled").into(),
        width,
        height,
        font_family: root
            .attribute("font-family")
            .unwrap_or("Noto Sans CJK SC")
            .into(),
        fonts: cx.fonts,
        assets: cx.assets,
        slides,
    })
}
/// Entry children that name another file.
fn references<'a, 'i>(root: X<'a, 'i>) -> impl Iterator<Item = X<'a, 'i>> {
    root.children().filter(|n| {
        n.is_element()
            && (n.tag_name().name() == "Library"
                || n.tag_name().name() == "Slide" && n.attribute("src").is_some())
    })
}
/// A reference is only its `src`; the referenced file holds the content.
fn reference(n: X<'_, '_>) -> Result<String> {
    attrs(n, &["src"])?;
    if n.children()
        .any(|c| c.is_element() || c.is_text() && !c.text().unwrap_or("").trim().is_empty())
    {
        return Err(invalid(format!(
            "<{} src> cannot have content",
            n.tag_name().name()
        )));
    }
    project_path(required(n, "src")?)
}
/// Relative, normalized and confined to the entry's directory.
fn project_path(src: &str) -> Result<String> {
    let parts = src.split('/').filter(|p| *p != ".").collect::<Vec<_>>();
    if src.starts_with('/')
        || src.contains(['\\', ':', '\0'])
        || parts.is_empty()
        || parts.iter().any(|p| p.is_empty() || *p == "..")
    {
        return Err(invalid(format!(
            "{src}: references must be relative paths inside the project"
        )));
    }
    Ok(parts.join("/"))
}
fn document<'t>(file: &str, text: &'t str) -> Result<roxmltree::Document<'t>> {
    let fail = |line: u32, column: u32, message: String| {
        Error::Source(vec![SourceError {
            file: file.into(),
            line,
            column,
            message,
        }])
    };
    if text.len() > MAX_FILE {
        return Err(fail(1, 1, "file exceeds 4 MiB".into()));
    }
    // DTD/entity expansion is deliberately never admitted.
    if text.contains("<!DOCTYPE") || text.contains("<!ENTITY") {
        return Err(fail(1, 1, "DTD and entities are not allowed".into()));
    }
    let doc = roxmltree::Document::parse(text).map_err(|e| {
        let pos = e.pos();
        fail(pos.row, pos.col, format!("XML: {e}"))
    })?;
    if doc.descendants().count() > MAX_NODES {
        return Err(fail(1, 1, "too many document nodes".into()));
    }
    Ok(doc)
}
fn position(file: &str, n: X<'_, '_>) -> Source {
    let pos = n.document().text_pos_at(n.range().start);
    Source {
        file: file.into(),
        line: pos.row,
        column: pos.col,
    }
}
/// Collects located errors so one check reports every independent mistake.
#[derive(Default)]
struct Errors(Vec<SourceError>);
impl Errors {
    fn keep<T>(&mut self, source: Source, result: Result<T>) -> Option<T> {
        match result.map_err(|e| at(&source, e)) {
            Ok(v) => Some(v),
            Err(Error::Source(found)) => {
                self.0.extend(found);
                None
            }
            Err(_) => unreachable!("at() always locates"),
        }
    }
    fn finish(mut self) -> Result<()> {
        if self.0.is_empty() {
            return Ok(());
        }
        self.0.truncate(50);
        Err(Error::Source(self.0))
    }
}
fn unique_insert<V>(map: &mut BTreeMap<String, V>, k: &str, v: V) -> Result<()> {
    if map.insert(k.to_owned(), v).is_some() {
        Err(invalid(format!("duplicate definition {k}")))
    } else {
        Ok(())
    }
}
pub(crate) fn attrs(node: X<'_, '_>, allowed: &[&str]) -> Result<()> {
    for a in node.attributes() {
        if !allowed.contains(&a.name()) {
            return Err(invalid(format!(
                "unknown attribute {} on <{}>",
                a.name(),
                node.tag_name().name()
            )));
        }
    }
    Ok(())
}
fn required<'a>(n: X<'a, 'a>, key: &str) -> Result<&'a str> {
    n.attribute(key)
        .filter(|x| !x.is_empty())
        .ok_or_else(|| invalid(format!("<{}> requires {key}", n.tag_name().name())))
}
#[derive(Default)]
struct Context<'a> {
    /// File whose elements are being read, for source positions.
    file: String,
    tokens: BTreeMap<String, String>,
    classes: BTreeMap<String, String>,
    templates: BTreeMap<String, (X<'a, 'a>, String)>,
    fonts: Vec<FontSpec>,
    assets: BTreeMap<String, String>,
    slides: BTreeSet<String>,
    /// Element IDs are unique within one slide.
    ids: BTreeSet<String>,
    count: usize,
}
impl<'a> Context<'a> {
    fn source(&self, n: X<'_, '_>) -> Source {
        position(&self.file, n)
    }
    fn define(
        &mut self,
        n: X<'a, 'a>,
        entry: bool,
        styles: &mut Vec<(String, X<'a, 'a>)>,
    ) -> Result<()> {
        let tag = n.tag_name().name();
        if !entry && ["Library", "Slide"].contains(&tag) {
            return Err(invalid(
                "a <Library> holds only Token, Style, Component, Font and Asset",
            ));
        }
        match tag {
            "Token" => {
                attrs(n, &["name", "value"])?;
                unique_insert(
                    &mut self.tokens,
                    required(n, "name")?,
                    required(n, "value")?.to_owned(),
                )
            }
            "Style" => {
                attrs(n, &["name"])?;
                styles.push((self.file.clone(), n));
                unique_insert(
                    &mut self.classes,
                    required(n, "name")?,
                    n.text().unwrap_or("").to_owned(),
                )
            }
            "Component" => {
                attrs(n, &["name"])?;
                unique_insert(
                    &mut self.templates,
                    required(n, "name")?,
                    (n, self.file.clone()),
                )
            }
            "Font" => {
                attrs(n, &["family", "src"])?;
                self.fonts.push(FontSpec {
                    family: required(n, "family")?.into(),
                    src: required(n, "src")?.into(),
                });
                Ok(())
            }
            "Asset" => {
                attrs(n, &["id", "src"])?;
                unique_insert(
                    &mut self.assets,
                    required(n, "id")?,
                    required(n, "src")?.to_owned(),
                )
            }
            "Library" | "Slide" => Ok(()),
            other => Err(invalid(format!("unknown definition <{other}>"))),
        }
    }
    fn slide(&mut self, n: X<'_, '_>) -> Result<Slide> {
        attrs(n, &["id", "background"])?;
        let id = required(n, "id")?.to_owned();
        self.ids.clear();
        self.count = 0;
        self.claim(&id)?;
        if !self.slides.insert(id.clone()) {
            return Err(invalid(format!("duplicate slide id: {id}")));
        }
        let background = self.resolve(n.attribute("background").unwrap_or("#FFFFFF"))?;
        if crate::layout::background_image(&background).is_none() {
            crate::layout::paint(&background)?;
        }
        let nodes = self.nodes(n, "", &BTreeMap::new(), 0)?;
        Ok(Slide {
            id,
            background,
            nodes,
        })
    }
    fn nodes(
        &mut self,
        parent: X<'_, '_>,
        prefix: &str,
        params: &BTreeMap<String, String>,
        depth: usize,
    ) -> Result<Vec<Node>> {
        let mut errors = Errors::default();
        let mut nodes = Vec::new();
        for n in parent.children().filter(|n| n.is_element()) {
            let node = self.node(n, prefix, params, depth);
            nodes.extend(errors.keep(self.source(n), node));
        }
        errors.finish()?;
        Ok(nodes)
    }
    fn claim(&mut self, id: &str) -> Result<()> {
        if id.len() > 200
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "_-/.".contains(c))
            || !self.ids.insert(id.into())
        {
            return Err(invalid(format!("invalid or duplicate id: {id}")));
        }
        Ok(())
    }
    fn resolve(&self, s: &str) -> Result<String> {
        self.expand(s, 0)
    }
    /// Depth bounds cycles; the output bound stops exponential fan-out.
    fn expand(&self, s: &str, depth: usize) -> Result<String> {
        if depth > 16 {
            return Err(invalid("cyclic or too deeply nested token reference"));
        }
        let mut out = String::new();
        let mut rest = s;
        while let Some(start) = rest.find("var(--") {
            let end = rest[start..]
                .find(')')
                .ok_or_else(|| invalid("unterminated token reference"))?
                + start;
            let key = &rest[start + 6..end];
            let v = self
                .tokens
                .get(key)
                .ok_or_else(|| invalid(format!("unknown token --{key}")))?;
            out.push_str(&rest[..start]);
            out.push_str(&self.expand(v, depth + 1)?);
            if out.len() > 64 * 1024 {
                return Err(invalid("token expansion exceeds 64 KiB"));
            }
            rest = &rest[end + 1..];
        }
        out.push_str(rest);
        Ok(out)
    }
    fn css(&self, s: &str) -> Result<Css> {
        let resolved = self.resolve(s)?;
        let mut map = Css::new();
        for part in resolved.split(';').filter(|x| !x.trim().is_empty()) {
            let (k, v) = part
                .split_once(':')
                .ok_or_else(|| invalid(format!("invalid style {part}")))?;
            let k = k.trim();
            let v = v.trim();
            if !PROPS.contains(&k) {
                return Err(invalid(format!("unsupported CSS property {k}")));
            }
            if v.is_empty() {
                return Err(invalid(format!("empty CSS value for {k}")));
            }
            declare(&mut map, k, v)?;
        }
        Ok(map)
    }
    fn node(
        &mut self,
        n: X<'_, '_>,
        prefix: &str,
        params: &BTreeMap<String, String>,
        depth: usize,
    ) -> Result<Node> {
        self.count += 1;
        if depth > 32 || self.count > MAX_NODES {
            return Err(invalid("component/tree expansion limit exceeded"));
        }
        let tag = n.tag_name().name();
        if tag == "Use" {
            let name = required(n, "component")?;
            let (template, file) = self
                .templates
                .get(name)
                .cloned()
                .ok_or_else(|| invalid(format!("unknown component {name}")))?;
            let instance = required(n, "id")?;
            let mut args = BTreeMap::new();
            for a in n
                .attributes()
                .filter(|a| a.name() != "component" && a.name() != "id")
            {
                args.insert(a.name().into(), a.value().into());
            }
            let roots = template
                .children()
                .filter(|n| n.is_element())
                .collect::<Vec<_>>();
            if roots.len() != 1 {
                return Err(invalid("Component requires exactly one root"));
            }
            // Positions inside the template belong to the file that defines it.
            let used_in = std::mem::replace(&mut self.file, file);
            let node = self.node(roots[0], &format!("{prefix}{instance}/"), &args, depth + 1);
            self.file = used_in;
            return node;
        }
        let mut allowed = vec!["id", "class", "style"];
        allowed.extend_from_slice(match tag {
            "Image" => &["src", "shape", "adjust"][..],
            "Icon" => &["src"],
            "Shape" => &["shape", "adjust"],
            "Path" => &["d", "viewBox"],
            "Table" => &["columns"],
            "Chart" => &[
                "type",
                "orient",
                "stack",
                "legend",
                "data-labels",
                "min",
                "max",
                "step",
                "format",
                "grid",
                "title",
                "smooth",
                "markers",
            ],
            _ => &[],
        });
        attrs(n, &allowed)?;
        let id = format!("{prefix}{}", required(n, "id")?);
        self.claim(&id)?;
        let style = self.style_of(n, params)?;
        let kind = match tag {
            "Box" => Kind::Box,
            "Group" => Kind::Group,
            "Text" => Kind::Text {
                paragraphs: self.paragraphs(n, params)?,
            },
            "Image" => Kind::Image {
                src: substitute(required(n, "src")?, params)?,
                clip: shape_kind(n, params)?,
            },
            "Icon" => Kind::Icon {
                src: substitute(required(n, "src")?, params)?,
            },
            "Shape" => Kind::Shape {
                shape: shape_kind(n, params)?,
            },
            "Path" => {
                let vals = n
                    .attribute("viewBox")
                    .unwrap_or("100 100")
                    .split_whitespace()
                    .map(|s| s.parse::<f32>().map_err(|_| invalid("invalid viewBox")))
                    .collect::<Result<Vec<_>>>()?;
                if vals.len() != 2 || vals.iter().any(|v| !v.is_finite() || *v <= 0.0) {
                    return Err(invalid("viewBox must contain two positive numbers"));
                }
                let data = required(n, "d")?.to_owned();
                validate_path(&data)?;
                Kind::Path {
                    data,
                    view_box: [vals[0], vals[1]],
                }
            }
            "Table" => {
                let (rows, columns) = self.table(n, params)?;
                Kind::Table { rows, columns }
            }
            "Chart" => Kind::Chart(Box::new(chart(n, params)?)),
            _ => return Err(invalid(format!("unknown element <{tag}>"))),
        };
        let children = if matches!(kind, Kind::Box | Kind::Group) {
            self.nodes(n, prefix, params, depth + 1)?
        } else {
            Vec::new()
        };
        if matches!(
            kind,
            Kind::Image { .. } | Kind::Icon { .. } | Kind::Shape { .. } | Kind::Path { .. }
        ) && n
            .children()
            .any(|c| c.is_element() || c.is_text() && !c.text().unwrap_or("").trim().is_empty())
        {
            return Err(invalid("leaf element cannot contain children"));
        }
        if matches!(kind, Kind::Box | Kind::Group)
            && n.children()
                .any(|c| c.is_text() && !c.text().unwrap_or("").trim().is_empty())
        {
            return Err(invalid("place text inside Text elements"));
        }
        Ok(Node {
            id,
            style,
            kind,
            children,
            source: self.source(n),
        })
    }
    /// A Text holds either inline content (one paragraph) or only `<P>` and `<List>` blocks.
    fn paragraphs(
        &self,
        n: X<'_, '_>,
        params: &BTreeMap<String, String>,
    ) -> Result<Vec<Paragraph>> {
        let blocks = n
            .children()
            .any(|c| c.is_element() && ["P", "List"].contains(&c.tag_name().name()));
        if !blocks {
            return Ok(vec![Paragraph {
                spans: self.inline(n, params)?,
                style: Css::new(),
                list: None,
            }]);
        }
        let mut out = Vec::new();
        for c in n.children() {
            if c.is_text() && !c.text().unwrap_or("").trim().is_empty() {
                return Err(invalid("text beside <P> or <List> must be inside a <P>"));
            }
            if !c.is_element() {
                continue;
            }
            match c.tag_name().name() {
                "P" => {
                    attrs(c, &["style"])?;
                    out.push(Paragraph {
                        spans: self.inline(c, params)?,
                        style: self.block_style(c, params)?,
                        list: None,
                    });
                }
                "List" => self.list(c, params, 0, &mut out)?,
                other => {
                    return Err(invalid(format!(
                        "<{other}> cannot sit beside <P> or <List>; wrap inline content in <P>"
                    )));
                }
            }
        }
        Ok(out)
    }
    /// `class` styles, then the inline `style`.
    fn style_of(&self, n: X<'_, '_>, params: &BTreeMap<String, String>) -> Result<Css> {
        let mut style = Css::new();
        if let Some(class) = n.attribute("class") {
            for name in class.split_whitespace() {
                style.extend(
                    self.css(
                        self.classes
                            .get(name)
                            .ok_or_else(|| invalid(format!("unknown class {name}")))?,
                    )?,
                );
            }
        }
        if let Some(s) = n.attribute("style") {
            style.extend(self.css(&substitute(s, params)?)?);
        }
        Ok(style)
    }
    /// Rows of Cells; spans must tile the column grid exactly.
    fn table(
        &self,
        n: X<'_, '_>,
        params: &BTreeMap<String, String>,
    ) -> Result<(Vec<TableRow>, Vec<f32>)> {
        let mut rows = Vec::new();
        for row in n.children() {
            if row.is_text() && !row.text().unwrap_or("").trim().is_empty() {
                return Err(invalid("Table text must be inside a Cell"));
            }
            if !row.is_element() {
                continue;
            }
            if row.tag_name().name() != "Row" {
                return Err(invalid("Table children must be Row"));
            }
            attrs(row, &["style", "class"])?;
            let style = self.style_of(row, params)?;
            if let Some(k) = style
                .keys()
                .find(|k| *k != "height" && !CELL_PROPS.contains(&k.as_str()))
            {
                return Err(invalid(format!("Row style does not take {k}")));
            }
            let mut cells = Vec::new();
            for c in row.children() {
                if c.is_text() && !c.text().unwrap_or("").trim().is_empty() {
                    return Err(invalid("Row text must be inside a Cell"));
                }
                if !c.is_element() {
                    continue;
                }
                if c.tag_name().name() != "Cell" {
                    return Err(invalid("Row children must be Cell"));
                }
                attrs(c, &["style", "class", "colspan", "rowspan"])?;
                let style = self.style_of(c, params)?;
                if let Some(k) = style.keys().find(|k| !CELL_PROPS.contains(&k.as_str())) {
                    return Err(invalid(format!("Cell style does not take {k}")));
                }
                let span = |k: &str| {
                    c.attribute(k)
                        .map_or(Some(1), |v| v.parse::<usize>().ok())
                        .filter(|v| (1..=50).contains(v))
                        .ok_or_else(|| invalid(format!("{k} must be 1–50")))
                };
                cells.push(TableCell {
                    paragraphs: self.paragraphs(c, params)?,
                    style,
                    colspan: span("colspan")?,
                    rowspan: span("rowspan")?,
                });
            }
            rows.push(TableRow { style, cells });
        }
        let first = rows
            .first()
            .map(|r| r.cells.iter().map(|c| c.colspan).sum::<usize>())
            .unwrap_or(0);
        let columns = match n.attribute("columns") {
            Some(s) => s
                .split_whitespace()
                .map(|x| {
                    x.parse::<f32>()
                        .ok()
                        .filter(|v| v.is_finite() && *v > 0.0)
                        .ok_or_else(|| invalid("table column ratios must be positive numbers"))
                })
                .collect::<Result<Vec<_>>>()?,
            None => vec![1.0; first],
        };
        if rows.is_empty() || columns.is_empty() || rows.len() > 200 || columns.len() > 50 {
            return Err(invalid("a Table needs 1–200 rows and 1–50 columns"));
        }
        // Each row must fill every column not covered by a rowspan from above.
        let mut covered = vec![vec![false; columns.len()]; rows.len()];
        for (r, row) in rows.iter().enumerate() {
            let mut col = 0;
            for cell in &row.cells {
                while col < columns.len() && covered[r][col] {
                    col += 1;
                }
                if col + cell.colspan > columns.len() || r + cell.rowspan > rows.len() {
                    return Err(invalid(format!(
                        "row {} has a cell spanning past the table's {} columns or {} rows",
                        r + 1,
                        columns.len(),
                        rows.len()
                    )));
                }
                for line in covered.iter_mut().skip(r).take(cell.rowspan) {
                    for slot in line.iter_mut().skip(col).take(cell.colspan) {
                        if *slot {
                            return Err(invalid(format!("row {} has overlapping cells", r + 1)));
                        }
                        *slot = true;
                    }
                }
                col += cell.colspan;
            }
            if covered[r].iter().any(|c| !c) {
                return Err(invalid(format!(
                    "row {} does not fill all {} columns",
                    r + 1,
                    columns.len()
                )));
            }
        }
        Ok((rows, columns))
    }
    fn block_style(&self, n: X<'_, '_>, params: &BTreeMap<String, String>) -> Result<Css> {
        let style = self.css(&substitute(n.attribute("style").unwrap_or(""), params)?)?;
        if style.keys().any(|k| !PARAGRAPH_PROPS.contains(&k.as_str())) {
            return Err(invalid(format!(
                "<{}> style accepts only text-align and margin-top",
                n.tag_name().name()
            )));
        }
        Ok(style)
    }
    /// Flattens a list into one paragraph per item; nested lists follow their item.
    fn list(
        &self,
        n: X<'_, '_>,
        params: &BTreeMap<String, String>,
        level: u8,
        out: &mut Vec<Paragraph>,
    ) -> Result<()> {
        attrs(n, &["type", "start", "style"])?;
        if level > 4 {
            return Err(invalid("lists nest at most 5 levels"));
        }
        let ordered = match n.attribute("type").unwrap_or("bullet") {
            "bullet" => false,
            "number" => true,
            other => {
                return Err(invalid(format!(
                    "List type is bullet or number, not {other}"
                )));
            }
        };
        let mut number = n
            .attribute("start")
            .map(|v| v.parse::<u32>().ok().filter(|v| (1..=9999).contains(v)))
            .unwrap_or(Some(1))
            .ok_or_else(|| invalid("List start must be 1–9999"))?;
        let mut style = self.block_style(n, params)?;
        // A list's margin-top separates the list from what precedes it, not each item.
        let mut before = style.remove("margin-top");
        for item in n.children() {
            if item.is_text() && !item.text().unwrap_or("").trim().is_empty() {
                return Err(invalid("List text must be inside <Item>"));
            }
            if !item.is_element() {
                continue;
            }
            if item.tag_name().name() != "Item" {
                return Err(invalid("List children must be <Item>"));
            }
            attrs(item, &["style"])?;
            let mut item_style = style.clone();
            if let Some(margin) = before.take() {
                item_style.insert("margin-top".into(), margin);
            }
            item_style.extend(self.block_style(item, params)?);
            out.push(Paragraph {
                spans: self.inline(item, params)?,
                style: item_style,
                list: Some(ListMarker {
                    ordered,
                    level,
                    number,
                }),
            });
            number += 1;
            for nested in item
                .children()
                .filter(|c| c.is_element() && c.tag_name().name() == "List")
            {
                self.list(nested, params, level + 1, out)?;
            }
        }
        Ok(())
    }
    /// Inline content: text, `<Span>`, `<A>` and `<br/>`. A nested `<List>` belongs to the
    /// enclosing `<Item>` and is read by [`Self::list`].
    fn inline(&self, n: X<'_, '_>, params: &BTreeMap<String, String>) -> Result<Vec<Span>> {
        let span = |text: String, style: Css, href: Option<String>| Span { text, style, href };
        let mut out = Vec::new();
        for c in n.children() {
            if c.is_text() {
                out.push(span(
                    substitute(c.text().unwrap_or(""), params)?,
                    Css::new(),
                    None,
                ));
            } else if c.is_element() {
                match c.tag_name().name() {
                    "br" => {
                        attrs(c, &[])?;
                        out.push(span("\n".into(), Css::new(), None));
                    }
                    "Span" | "A" => {
                        let link = c.tag_name().name() == "A";
                        attrs(c, if link { &["href", "style"] } else { &["style"] })?;
                        if c.children().any(|n| n.is_element()) {
                            return Err(invalid(
                                "Span and A do not nest elements; use adjacent spans",
                            ));
                        }
                        let mut style =
                            self.css(&substitute(c.attribute("style").unwrap_or(""), params)?)?;
                        if style.keys().any(|k| !SPAN_PROPS.contains(&k.as_str())) {
                            return Err(invalid("Span and A accept text style properties only"));
                        }
                        let href = if link {
                            if style.contains_key("color") || style.contains_key("text-decoration")
                            {
                                return Err(invalid(
                                    "links use the theme link color and underline; A cannot set color or text-decoration",
                                ));
                            }
                            style.insert("color".into(), LINK_COLOR.into());
                            style.insert("text-decoration".into(), "underline".into());
                            Some(link_target(&substitute(required(c, "href")?, params)?)?)
                        } else {
                            None
                        };
                        out.push(span(substitute(&plain(c), params)?, style, href));
                    }
                    "List" if n.tag_name().name() == "Item" => {}
                    other => return Err(invalid(format!("unsupported rich-text tag {other}"))),
                }
            }
        }
        // Source line breaks and indentation around the content are formatting, not text.
        let formatting = |s: &str| s.contains('\n') && s.trim().is_empty();
        if let Some(first) = out.first_mut() {
            let lead = first.text.len() - first.text.trim_start().len();
            if formatting(&first.text[..lead]) {
                first.text.drain(..lead);
            }
        }
        // Trailing formatting may span several text nodes, e.g. before a nested <List>.
        while let Some(last) = out.last_mut() {
            let kept = last.text.trim_end().len();
            if formatting(&last.text[kept..]) {
                last.text.truncate(kept);
            }
            if last.text.is_empty() && out.len() > 1 {
                out.pop();
            } else {
                break;
            }
        }
        Ok(out)
    }
}
/// The PPTX theme's hyperlink color; Office paints links with it.
pub(crate) const LINK_COLOR: &str = "#2563EB";
pub(crate) fn link_target(href: &str) -> Result<String> {
    let lower = href.to_ascii_lowercase();
    if href.len() > 2048
        || href.chars().any(char::is_whitespace)
        || !["https://", "http://", "mailto:"]
            .iter()
            .any(|scheme| lower.starts_with(scheme) && lower.len() > scheme.len())
    {
        return Err(invalid(format!(
            "link href must be an http, https or mailto address: {href}"
        )));
    }
    Ok(href.into())
}
/// Stores only longhands, so a later declaration overrides an earlier one as in CSS.
fn declare(map: &mut Css, k: &str, v: &str) -> Result<()> {
    let p = v.split_whitespace().collect::<Vec<_>>();
    let mut set = |k: &str, v: &str| map.insert(k.into(), v.into());
    match k {
        "padding" | "margin" => {
            let [t, r, b, l] = match p[..] {
                [a] => [a, a, a, a],
                [t, r] => [t, r, t, r],
                [t, r, b] => [t, r, b, r],
                [t, r, b, l] => [t, r, b, l],
                _ => return Err(invalid(format!("{k} expects 1–4 lengths"))),
            };
            for (side, v) in ["top", "right", "bottom", "left"]
                .into_iter()
                .zip([t, r, b, l])
            {
                set(&format!("{k}-{side}"), v);
            }
        }
        "gap" => {
            let [row, column] = match p[..] {
                [a] => [a, a],
                [row, column] => [row, column],
                _ => return Err(invalid("gap expects 1–2 lengths")),
            };
            set("row-gap", row);
            set("column-gap", column);
        }
        "border" | "border-top" | "border-right" | "border-bottom" | "border-left" => {
            let [width, style @ ("solid" | "dashed" | "dotted"), color] = p[..] else {
                return Err(invalid(format!(
                    "{k} syntax: <length> solid|dashed|dotted <color>"
                )));
            };
            set(&format!("{k}-width"), width);
            set(&format!("{k}-style"), style);
            set(&format!("{k}-color"), color);
        }
        "background-color" => {
            set("background", v);
        }
        _ => {
            set(k, v);
        }
    }
    Ok(())
}
/// `shape` (an OOXML preset, default rect) and `adjust` attributes of a Shape or Image.
fn shape_kind(n: X<'_, '_>, params: &BTreeMap<String, String>) -> Result<ShapeKind> {
    let preset = n.attribute("shape").unwrap_or("rect");
    if !crate::preset::exists(preset) {
        return Err(invalid(format!(
            "unknown {} shape {preset}; use an OOXML preset name such as rect, ellipse, line, triangle, rightArrow, star5 or wedgeRoundRectCallout",
            n.tag_name().name()
        )));
    }
    let adjust = substitute(n.attribute("adjust").unwrap_or(""), params)?
        .split_whitespace()
        .map(|v| {
            v.parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(|| invalid(format!("invalid adjust value {v}")))
        })
        .collect::<Result<Vec<_>>>()?;
    let names = crate::preset::adjust_names(preset);
    if adjust.len() > names.len() {
        return Err(invalid(format!(
            "{preset} takes at most {} adjust values ({})",
            names.len(),
            names.join(" ")
        )));
    }
    Ok(ShapeKind {
        preset: preset.into(),
        adjust,
    })
}
fn chart(n: X<'_, '_>, params: &BTreeMap<String, String>) -> Result<Chart> {
    let get = |k: &str| n.attribute(k).map(|v| substitute(v, params)).transpose();
    let flag = |k: &str, default: bool| -> Result<bool> {
        match get(k)?.as_deref() {
            None => Ok(default),
            Some("false") => Ok(false),
            Some("true") => Ok(true),
            Some(v) => Err(invalid(format!("Chart {k} must be true or false, not {v}"))),
        }
    };
    let number = |k: &str| -> Result<Option<f64>> {
        get(k)?
            .map(|v| {
                v.trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|x| x.is_finite())
                    .ok_or_else(|| invalid(format!("Chart {k} must be a number")))
            })
            .transpose()
    };
    let kind: ChartType = keyword(get("type")?.as_deref().unwrap_or("bar"), "Chart type")?;
    let mut categories = Vec::new();
    let mut series = Vec::new();
    for c in n.children().filter(|x| x.is_element()) {
        match c.tag_name().name() {
            "Labels" => {
                attrs(c, &[])?;
                categories = substitute(&plain(c), params)?
                    .split('|')
                    .map(|s| s.trim().to_owned())
                    .collect();
            }
            "Series" => {
                attrs(c, &["name", "color", "colors", "type"])?;
                let values = substitute(&plain(c), params)?
                    .split_whitespace()
                    .map(|s| {
                        s.parse::<f64>()
                            .ok()
                            .filter(|v| v.is_finite())
                            .ok_or_else(|| invalid(format!("invalid chart number {s}")))
                    })
                    .collect::<Result<Vec<_>>>()?;
                let own: Option<ChartType> = c
                    .attribute("type")
                    .map(|v| keyword(v, "Series type"))
                    .transpose()?;
                if let Some(t) = own
                    && (!matches!(kind, ChartType::Bar | ChartType::Line | ChartType::Area)
                        || !matches!(t, ChartType::Bar | ChartType::Line | ChartType::Area))
                {
                    return Err(invalid(
                        "Series type combines bar, line and area series in a bar, line or area chart",
                    ));
                }
                let colors = match (c.attribute("color"), c.attribute("colors")) {
                    (Some(_), Some(_)) => {
                        return Err(invalid("a Series takes color or colors, not both"));
                    }
                    (Some(v), None) if !kind.is_round() => {
                        vec![Color::parse(&substitute(v, params)?)?]
                    }
                    (None, Some(v)) if kind.is_round() => substitute(v, params)?
                        .split_whitespace()
                        .map(Color::parse)
                        .collect::<Result<Vec<_>>>()?,
                    (None, None) => Vec::new(),
                    _ => {
                        return Err(invalid(
                            "pie and doughnut Series take colors (one per slice); other Series take color",
                        ));
                    }
                };
                series.push(Series {
                    name: substitute(required(c, "name")?, params)?,
                    values,
                    kind: own.unwrap_or(kind),
                    colors,
                });
            }
            _ => return Err(invalid("Chart contains Labels and Series")),
        }
    }
    let round = kind.is_round();
    let has = |t: ChartType| series.iter().any(|s| s.kind == t);
    let rule = |ok: bool, message: &str| if ok { Ok(()) } else { Err(invalid(message)) };
    rule(
        !categories.is_empty() && categories.len() <= 100,
        "a Chart needs 1 to 100 Labels",
    )?;
    rule(
        !series.is_empty() && series.len() <= 8,
        "a Chart needs 1 to 8 Series",
    )?;
    rule(
        series.iter().all(|s| s.values.len() == categories.len()),
        "every Series needs one value per label",
    )?;
    rule(
        !round || series.len() == 1,
        "pie and doughnut charts take one Series",
    )?;
    rule(
        series
            .iter()
            .all(|s| s.colors.len() <= 1 || s.colors.len() == categories.len()),
        "colors needs one color per slice",
    )?;
    if kind == ChartType::Scatter {
        rule(
            categories
                .iter()
                .all(|c| c.parse::<f64>().is_ok_and(f64::is_finite)),
            "scatter chart Labels are numeric x values",
        )?;
    }
    let horizontal = match get("orient")?.as_deref() {
        None | Some("vertical") => false,
        Some("horizontal") => true,
        Some(v) => {
            return Err(invalid(format!(
                "Chart orient must be vertical or horizontal, not {v}"
            )));
        }
    };
    rule(
        !horizontal || series.iter().all(|s| s.kind == ChartType::Bar),
        "orient=\"horizontal\" is for bar charts without line or area series",
    )?;
    let stack: Stack = keyword(get("stack")?.as_deref().unwrap_or("none"), "Chart stack")?;
    rule(
        stack == Stack::None || matches!(kind, ChartType::Bar | ChartType::Area),
        "stack is for bar and area charts",
    )?;
    let negative = series.iter().flat_map(|s| &s.values).any(|v| *v < 0.0);
    rule(
        !negative || !round && stack != Stack::Percent,
        "pie, doughnut and percent-stacked values cannot be negative",
    )?;
    rule(
        !round || series[0].values.iter().sum::<f64>() > 0.0,
        "pie and doughnut values need a positive total",
    )?;
    let legend = match get("legend")?.as_deref() {
        None | Some("auto") => (round || series.len() > 1).then_some(Side::Bottom),
        Some("none") => None,
        Some(v) => Some(keyword(v, "Chart legend")?),
    };
    let data_labels: DataLabels = keyword(
        get("data-labels")?.as_deref().unwrap_or("none"),
        "Chart data-labels",
    )?;
    rule(
        round || !matches!(data_labels, DataLabels::Percent | DataLabels::Category),
        "data-labels percent and category are for pie and doughnut charts",
    )?;
    rule(
        data_labels == DataLabels::None || !has(ChartType::Area),
        "data-labels do not apply to area series",
    )?;
    rule(
        stack != Stack::Percent || series.iter().all(|s| s.kind == kind),
        "percent stacking cannot mix series types",
    )?;
    let (min, max, step) = (number("min")?, number("max")?, number("step")?);
    rule(
        !round || min.is_none() && max.is_none() && step.is_none() && get("grid")?.is_none(),
        "pie and doughnut charts have no axis: min, max, step and grid do not apply",
    )?;
    rule(
        stack != Stack::Percent || min.is_none() && max.is_none() && step.is_none(),
        "a percent-stacked axis always runs 0% to 100%",
    )?;
    rule(
        min.zip(max).is_none_or(|(a, b)| a < b),
        "Chart min must be below max",
    )?;
    rule(step.is_none_or(|s| s > 0.0), "Chart step must be positive")?;
    let format = get("format")?.unwrap_or_else(|| "General".into());
    crate::chart::format_value(0.0, &format)?;
    let lines = has(ChartType::Line);
    let (smooth, markers) = (flag("smooth", false)?, flag("markers", false)?);
    rule(
        lines || !smooth && !markers,
        "smooth and markers are for line series",
    )?;
    Ok(Chart {
        kind,
        horizontal,
        stack,
        categories,
        series,
        legend,
        data_labels,
        min,
        max,
        step,
        format,
        grid: !round && flag("grid", true)?,
        title: get("title")?.filter(|t| !t.trim().is_empty()),
        smooth,
        markers,
    })
}
fn plain(n: X<'_, '_>) -> String {
    n.descendants()
        .filter(|n| n.is_text())
        .filter_map(|n| n.text())
        .collect()
}
fn substitute(s: &str, params: &BTreeMap<String, String>) -> Result<String> {
    let mut out = s.to_owned();
    for (k, v) in params {
        out = out.replace(&format!("${{{k}}}"), v);
    }
    if out.contains("${") {
        return Err(invalid("unresolved component parameter"));
    }
    Ok(out)
}
/// Full SVG path data: relative commands, H/V, S/T, Q and arcs are all accepted.
fn validate_path(data: &str) -> Result<()> {
    crate::icon::path_commands(data).map(|_| ())
}
