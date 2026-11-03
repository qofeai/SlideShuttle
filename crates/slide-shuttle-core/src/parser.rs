use crate::model::*;
use roxmltree::Node as X;
use std::collections::{BTreeMap, BTreeSet};

const MAX_SOURCE: usize = 4 * 1024 * 1024;
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
    "object-fit",
    "box-sizing",
    "white-space",
];

pub fn parse(source: &str) -> Result<Deck> {
    if source.len() > MAX_SOURCE {
        return Err(invalid("source exceeds 4 MiB"));
    }
    // DTD/entity expansion is deliberately never admitted.
    if source.contains("<!DOCTYPE") || source.contains("<!ENTITY") {
        return Err(invalid("DTD and entities are not allowed"));
    }
    let doc = roxmltree::Document::parse(source)?;
    if doc.descendants().count() > MAX_NODES {
        return Err(invalid("too many document nodes"));
    }
    let root = doc.root_element();
    if root.tag_name().name() != "Deck" {
        return Err(invalid("root must be <Deck version=\"1\">"));
    }
    attrs(
        root,
        &["version", "title", "width", "height", "font-family"],
    )?;
    if root.attribute("version") != Some(FORMAT_VERSION) {
        return Err(invalid("unsupported Deck version; expected 1"));
    }
    let width = length_px(root.attribute("width").unwrap_or("1280px"))?;
    let height = length_px(root.attribute("height").unwrap_or("720px"))?;
    if width <= 0.0 || height <= 0.0 || width > 16384.0 || height > 16384.0 {
        return Err(invalid("page size must be in (0, 16384]px"));
    }
    let mut cx = Context::default();
    let mut errors = Errors::default();
    for n in root.children().filter(|n| n.is_element()) {
        let defined = cx.define(n);
        errors.keep(n, defined);
    }
    for n in root
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "Style")
    {
        let checked = cx.css(n.text().unwrap_or(""));
        errors.keep(n, checked);
    }
    let mut slides = Vec::new();
    for n in root
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "Slide")
    {
        let slide = cx.slide(n);
        slides.extend(errors.keep(n, slide));
    }
    errors.finish()?;
    if slides.is_empty() || slides.len() > 300 {
        return Err(invalid("Deck requires 1–300 slides"));
    }
    Ok(Deck {
        version: FORMAT_VERSION.into(),
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
/// Collects located errors so one check reports every independent mistake.
#[derive(Default)]
struct Errors(Vec<SourceError>);
impl Errors {
    fn keep<T>(&mut self, n: X<'_, '_>, result: Result<T>) -> Option<T> {
        match result.map_err(|e| at(&source(n), e)) {
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
fn source(n: X<'_, '_>) -> Source {
    let pos = n.document().text_pos_at(n.range().start);
    Source {
        line: pos.row,
        column: pos.col,
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
    tokens: BTreeMap<String, String>,
    classes: BTreeMap<String, String>,
    templates: BTreeMap<String, X<'a, 'a>>,
    fonts: Vec<FontSpec>,
    assets: BTreeMap<String, String>,
    ids: BTreeSet<String>,
    count: usize,
}
impl<'a> Context<'a> {
    fn define(&mut self, n: X<'a, 'a>) -> Result<()> {
        match n.tag_name().name() {
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
                unique_insert(
                    &mut self.classes,
                    required(n, "name")?,
                    n.text().unwrap_or("").to_owned(),
                )
            }
            "Component" => {
                attrs(n, &["name"])?;
                unique_insert(&mut self.templates, required(n, "name")?, n)
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
            "Slide" => Ok(()),
            other => Err(invalid(format!("unknown Deck child <{other}>"))),
        }
    }
    fn slide(&mut self, n: X<'_, '_>) -> Result<Slide> {
        attrs(n, &["id", "background"])?;
        let id = required(n, "id")?.to_owned();
        self.claim(&id)?;
        let background = self.resolve(n.attribute("background").unwrap_or("#FFFFFF"))?;
        Color::parse(&background)?;
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
            nodes.extend(errors.keep(n, node));
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
            let template = *self
                .templates
                .get(name)
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
            return self.node(roots[0], &format!("{prefix}{instance}/"), &args, depth + 1);
        }
        let mut allowed = vec!["id", "class", "style"];
        allowed.extend_from_slice(match tag {
            "Image" => &["src"][..],
            "Shape" => &["shape"],
            "Path" => &["d", "viewBox"],
            "Table" => &["columns"],
            "Chart" => &["type"],
            _ => &[],
        });
        attrs(n, &allowed)?;
        let id = format!("{prefix}{}", required(n, "id")?);
        self.claim(&id)?;
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
        let kind = match tag {
            "Box" => Kind::Box,
            "Group" => Kind::Group,
            "Text" => Kind::Text {
                spans: self.spans(n, params)?,
            },
            "Image" => Kind::Image {
                src: substitute(required(n, "src")?, params)?,
            },
            "Shape" => Kind::Shape {
                shape: keyword(n.attribute("shape").unwrap_or("rect"), "Shape shape")?,
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
                let mut rows = Vec::new();
                for row in n.children().filter(|x| x.is_element()) {
                    if row.tag_name().name() != "Row" {
                        return Err(invalid("Table children must be Row"));
                    }
                    attrs(row, &[])?;
                    let mut cells = Vec::new();
                    for c in row.children().filter(|x| x.is_element()) {
                        if c.tag_name().name() != "Cell" {
                            return Err(invalid("Row children must be Cell"));
                        }
                        attrs(c, &[])?;
                        cells.push(substitute(&plain(c), params)?);
                    }
                    rows.push(cells);
                }
                if rows.is_empty()
                    || rows[0].is_empty()
                    || rows.iter().any(|r| r.len() != rows[0].len())
                {
                    return Err(invalid("Table requires rectangular, nonempty rows"));
                }
                let columns = n
                    .attribute("columns")
                    .map(|s| {
                        s.split_whitespace()
                            .map(|x| {
                                x.parse::<f32>()
                                    .map_err(|_| invalid("invalid table column ratio"))
                            })
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_else(|| vec![1.0; rows[0].len()]);
                if columns.len() != rows[0].len()
                    || columns.iter().any(|v| !v.is_finite() || *v <= 0.0)
                {
                    return Err(invalid("invalid table column ratios"));
                }
                Kind::Table { rows, columns }
            }
            "Chart" => {
                let chart_type: ChartType =
                    keyword(n.attribute("type").unwrap_or("bar"), "Chart type")?;
                let mut labels = Vec::new();
                let mut series = Vec::new();
                for c in n.children().filter(|x| x.is_element()) {
                    match c.tag_name().name() {
                        "Labels" => {
                            attrs(c, &[])?;
                            labels = plain(c).split('|').map(|s| s.trim().to_owned()).collect();
                        }
                        "Series" => {
                            attrs(c, &["name", "color"])?;
                            let values = plain(c)
                                .split_whitespace()
                                .map(|s| {
                                    s.parse::<f64>()
                                        .map_err(|_| invalid("invalid chart number"))
                                })
                                .collect::<Result<Vec<_>>>()?;
                            series.push(Series {
                                name: required(c, "name")?.into(),
                                values,
                                color: c.attribute("color").unwrap_or("#2563EB").into(),
                            });
                        }
                        _ => return Err(invalid("Chart requires Labels and Series")),
                    }
                }
                if labels.is_empty()
                    || series.is_empty()
                    || series.len() > 8
                    || labels.len() > 100
                    || series.iter().any(|s| {
                        s.values.len() != labels.len() || s.values.iter().any(|v| !v.is_finite())
                    })
                {
                    return Err(invalid("invalid chart data dimensions"));
                }
                for s in &series {
                    Color::parse(&s.color)?;
                }
                Kind::Chart {
                    chart_type,
                    labels,
                    series,
                }
            }
            _ => return Err(invalid(format!("unknown element <{tag}>"))),
        };
        let children = if matches!(kind, Kind::Box | Kind::Group) {
            self.nodes(n, prefix, params, depth + 1)?
        } else {
            Vec::new()
        };
        if matches!(
            kind,
            Kind::Image { .. } | Kind::Shape { .. } | Kind::Path { .. }
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
            source: source(n),
        })
    }
    fn spans(&self, n: X<'_, '_>, params: &BTreeMap<String, String>) -> Result<Vec<Span>> {
        let mut out = Vec::new();
        for c in n.children() {
            if c.is_text() {
                out.push(Span {
                    text: substitute(c.text().unwrap_or(""), params)?,
                    style: Css::new(),
                });
            } else if c.is_element() {
                match c.tag_name().name() {
                    "br" => {
                        attrs(c, &[])?;
                        out.push(Span {
                            text: "\n".into(),
                            style: Css::new(),
                        });
                    }
                    "Span" => {
                        attrs(c, &["style"])?;
                        if c.children().any(|n| n.is_element()) {
                            return Err(invalid("Span does not nest elements; use adjacent spans"));
                        }
                        let style = self.css(c.attribute("style").unwrap_or(""))?;
                        if style.keys().any(|k| {
                            ![
                                "font-family",
                                "font-size",
                                "font-weight",
                                "font-style",
                                "line-height",
                                "color",
                            ]
                            .contains(&k.as_str())
                        }) {
                            return Err(invalid("Span accepts text style properties only"));
                        }
                        out.push(Span {
                            text: substitute(&plain(c), params)?,
                            style,
                        });
                    }
                    other => return Err(invalid(format!("unsupported rich-text tag {other}"))),
                }
            }
        }
        // XML formatting indentation around a multi-line Text is not visual content.
        if out.len() == 1 && out[0].text.starts_with('\n') {
            out[0].text = out[0].text.trim().to_owned();
        }
        Ok(out)
    }
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
        "border" => {
            let [width, "solid", color] = p[..] else {
                return Err(invalid("border syntax: <length> solid <hex>"));
            };
            set("border-width", width);
            set("border-color", color);
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
fn validate_path(data: &str) -> Result<()> {
    let mut count = 0;
    for seg in svgtypes::PathParser::from(data) {
        use svgtypes::PathSegment::*;
        let seg = seg.map_err(|e| invalid(format!("invalid Path: {e}")))?;
        if !matches!(
            seg,
            MoveTo { abs: true, .. }
                | LineTo { abs: true, .. }
                | CurveTo { abs: true, .. }
                | ClosePath { .. }
        ) {
            return Err(invalid("Path supports absolute M/L/C/Z only"));
        }
        count += 1;
    }
    if count == 0 || count > 10000 {
        return Err(invalid("Path must have 1–10000 segments"));
    }
    Ok(())
}
