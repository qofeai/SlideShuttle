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
    let mut tokens = BTreeMap::new();
    let mut classes = BTreeMap::new();
    let mut templates = BTreeMap::new();
    let mut fonts = Vec::new();
    let mut assets = BTreeMap::new();
    for n in root.children().filter(|n| n.is_element()) {
        match n.tag_name().name() {
            "Token" => {
                attrs(n, &["name", "value"])?;
                unique_insert(
                    &mut tokens,
                    required(n, "name")?,
                    required(n, "value")?.to_owned(),
                )?;
            }
            "Style" => {
                attrs(n, &["name"])?;
                unique_insert(
                    &mut classes,
                    required(n, "name")?,
                    n.text().unwrap_or("").to_owned(),
                )?;
            }
            "Component" => {
                attrs(n, &["name"])?;
                unique_insert(&mut templates, required(n, "name")?, n)?;
            }
            "Font" => {
                attrs(n, &["family", "src"])?;
                fonts.push(FontSpec {
                    family: required(n, "family")?.into(),
                    src: required(n, "src")?.into(),
                });
            }
            "Asset" => {
                attrs(n, &["id", "src"])?;
                unique_insert(
                    &mut assets,
                    required(n, "id")?,
                    required(n, "src")?.to_owned(),
                )?;
            }
            "Slide" => {}
            other => return Err(invalid(format!("unknown Deck child <{other}>"))),
        }
    }
    let mut cx = Context {
        tokens,
        classes,
        templates,
        ids: BTreeSet::new(),
        count: 0,
    };
    for definition in cx.classes.values() {
        cx.css(definition)?;
    }
    let mut slides = Vec::new();
    for n in root
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "Slide")
    {
        attrs(n, &["id", "background"])?;
        let id = required(n, "id")?.to_owned();
        cx.claim(&id)?;
        let background = cx.resolve(n.attribute("background").unwrap_or("#FFFFFF"))?;
        Color::parse(&background)?;
        let nodes = n
            .children()
            .filter(|n| n.is_element())
            .map(|n| cx.node(n, "", &BTreeMap::new(), 0))
            .collect::<Result<_>>()?;
        slides.push(Slide {
            id,
            background,
            nodes,
        });
    }
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
        fonts,
        assets,
        slides,
    })
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
struct Context<'a> {
    tokens: BTreeMap<String, String>,
    classes: BTreeMap<String, String>,
    templates: BTreeMap<String, X<'a, 'a>>,
    ids: BTreeSet<String>,
    count: usize,
}
impl Context<'_> {
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
        let mut out = s.to_owned();
        for _ in 0..16 {
            let Some(start) = out.find("var(--") else {
                return Ok(out);
            };
            let end = out[start..]
                .find(')')
                .ok_or_else(|| invalid("unterminated token reference"))?
                + start;
            let key = &out[start + 6..end];
            let v = self
                .tokens
                .get(key)
                .ok_or_else(|| invalid(format!("unknown token --{key}")))?;
            out.replace_range(start..=end, v);
        }
        Err(invalid("cyclic or excessive token expansion"))
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
            map.insert(k.into(), v.into());
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
        let pos = n.document().text_pos_at(n.range().start);
        let kind = match tag {
            "Box" => Kind::Box,
            "Group" => Kind::Group,
            "Text" => Kind::Text {
                spans: self.spans(n, params)?,
            },
            "Image" => Kind::Image {
                src: substitute(required(n, "src")?, params)?,
            },
            "Shape" => {
                let shape = n.attribute("shape").unwrap_or("rect");
                if !["rect", "ellipse", "line"].contains(&shape) {
                    return Err(invalid("Shape supports rect, ellipse, line"));
                }
                Kind::Shape {
                    shape: shape.into(),
                }
            }
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
                let chart_type = n.attribute("type").unwrap_or("bar");
                if !["bar", "line"].contains(&chart_type) {
                    return Err(invalid("Chart supports bar or line"));
                }
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
                    chart_type: chart_type.into(),
                    labels,
                    series,
                }
            }
            _ => return Err(invalid(format!("unknown element <{tag}>"))),
        };
        let children = if matches!(kind, Kind::Box | Kind::Group) {
            n.children()
                .filter(|n| n.is_element())
                .map(|n| self.node(n, prefix, params, depth + 1))
                .collect::<Result<Vec<_>>>()?
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
            source: Source {
                line: pos.row,
                column: pos.col,
            },
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
