//! Resolve supported appearance and text properties through their actual OOXML
//! relationships. The result is flattened into explicit values in our language.
use super::colors::{Problems, Theme, color_hex, percentage};
use super::{child, desc, plain, related_part};
use crate::{
    model::*,
    pptx::{A, P},
};
use roxmltree::{Document, Node as X};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub(super) struct Styles<'d, 'i> {
    pub theme: Theme<'i>,
    presentation: X<'d, 'i>,
    slide: X<'d, 'i>,
    inherited: Vec<X<'d, 'i>>,
}

impl<'d, 'i> Styles<'d, 'i> {
    pub fn new(
        entries: &'i BTreeMap<String, Vec<u8>>,
        presentation: X<'d, 'i>,
        slide: X<'d, 'i>,
        inherited: &'d [Document<'i>],
        parts: &[String],
    ) -> Result<Self> {
        let mut themes = Vec::new();
        // Broadest to closest: presentation, master, layout, slide. Overrides
        // replace the corresponding theme sections without assuming filenames.
        for part in
            std::iter::once("ppt/presentation.xml").chain(parts.iter().rev().map(String::as_str))
        {
            if part.is_empty() {
                continue;
            }
            for kind in ["theme", "themeOverride"] {
                if let Some(path) = related_part(entries, part, kind)? {
                    if let Some(index) = themes.iter().position(|p| p == &path) {
                        themes.remove(index);
                    }
                    themes.push(path);
                }
            }
        }
        let mut theme = Theme::new(entries, &themes)?;
        let inherited = inherited
            .iter()
            .map(Document::root_element)
            .collect::<Vec<_>>();
        if let Some(master) = inherited.last().and_then(|n| child(*n, P, "clrMap")) {
            for attr in master.attributes() {
                theme.map.insert(attr.name().into(), attr.value().into());
            }
        }
        let master_map = theme.map.clone();
        for root in inherited
            .iter()
            .rev()
            .filter(|n| !n.has_tag_name((P, "sldMaster")))
            .chain(std::iter::once(&slide))
        {
            if let Some(mapping) = child(*root, P, "clrMapOvr") {
                if child(mapping, A, "masterClrMapping").is_some() {
                    theme.map = master_map.clone();
                }
                if let Some(over) = child(mapping, A, "overrideClrMapping") {
                    for attr in over.attributes() {
                        theme.map.insert(attr.name().into(), attr.value().into());
                    }
                }
            }
        }
        Ok(Self {
            theme,
            presentation,
            slide,
            inherited,
        })
    }

    pub fn background(&self, problems: &mut Problems) -> String {
        for root in std::iter::once(self.slide).chain(self.inherited.iter().copied()) {
            let Some(bg) = child(root, P, "cSld").and_then(|n| child(n, P, "bg")) else {
                continue;
            };
            let color = if let Some(pr) = child(bg, P, "bgPr") {
                self.theme.fill(pr, None, problems)
            } else if let Some(reference) = child(bg, P, "bgRef") {
                self.fill_reference(reference, problems)
            } else {
                problems.insert("background has no supported fill".into());
                None
            };
            return color
                .map(|c| format!("#{}", color_hex(c)))
                .unwrap_or("#FFFFFF".into());
        }
        "#FFFFFF".into()
    }

    fn fill_reference(&self, reference: X<'_, '_>, problems: &mut Problems) -> Option<Color> {
        let Some(index) = reference
            .attribute("idx")
            .and_then(|s| s.parse::<usize>().ok())
        else {
            problems.insert("invalid theme fill reference index".into());
            return None;
        };
        if index == 0 || index == 1000 {
            return Some(Color(0, 0, 0, 0));
        }
        let (list, index) = if index >= 1001 {
            ("bgFillStyleLst", index - 1001)
        } else {
            ("fillStyleLst", index - 1)
        };
        let entry = self
            .theme
            .element("fmtScheme")
            .and_then(|n| child(n, A, list))
            .and_then(|n| n.children().filter(|n| n.is_element()).nth(index));
        let Some(entry) = entry else {
            problems.insert(format!(
                "theme fill reference {} is unavailable",
                reference.attribute("idx").unwrap_or("")
            ));
            return None;
        };
        let placeholder = self.theme.color_child(reference, None, problems);
        self.theme.fill(entry, placeholder, problems)
    }
    fn visual(&self, shape: X<'_, '_>, props: &mut Map<String, Value>, problems: &mut Problems) {
        if let Some(style) = child(shape, P, "style") {
            if let Some(fill) =
                child(style, A, "fillRef").and_then(|n| self.fill_reference(n, problems))
            {
                props.insert("fill".into(), color_hex(fill).into());
            }
            if let Some(reference) = child(style, A, "lnRef") {
                let index = reference
                    .attribute("idx")
                    .and_then(|s| s.parse::<usize>().ok());
                if index == Some(0) {
                    props.insert("strokeWidth".into(), 0.into());
                } else if let Some(index) = index {
                    if let Some(line) = self
                        .theme
                        .element("fmtScheme")
                        .and_then(|n| child(n, A, "lnStyleLst"))
                        .and_then(|n| n.children().filter(|n| n.is_element()).nth(index - 1))
                    {
                        let placeholder = self.theme.color_child(reference, None, problems);
                        self.line(line, placeholder, props, problems);
                    } else {
                        problems.insert(format!("theme line reference {index} is unavailable"));
                    }
                } else {
                    problems.insert("invalid theme line reference index".into());
                }
            }
        }
        if let Some(pr) = child(shape, P, "spPr") {
            if let Some(color) = self.theme.fill(pr, None, problems) {
                props.insert("fill".into(), color_hex(color).into());
            }
            if let Some(line) = child(pr, A, "ln") {
                self.line(line, None, props, problems);
            }
        }
    }
    fn line(
        &self,
        line: X<'_, '_>,
        placeholder: Option<Color>,
        props: &mut Map<String, Value>,
        problems: &mut Problems,
    ) {
        if let Some(width) = line.attribute("w").and_then(|s| s.parse::<u64>().ok()) {
            props.insert("strokeWidth".into(), width.into());
        }
        if let Some(color) = self.theme.fill(line, placeholder, problems) {
            if color.3 == 0 {
                props.insert("strokeWidth".into(), 0.into());
            } else if !props.contains_key("strokeWidth") {
                props.insert("strokeWidth".into(), 9525.into());
            }
            props.insert("stroke".into(), color_hex(color).into());
        }
    }

    fn chain<'s>(&'s self, shape: X<'s, 'i>) -> Vec<X<'s, 'i>> {
        let Some(ph) = desc(shape, P, "ph") else {
            return vec![shape];
        };
        let idx = ph.attribute("idx").unwrap_or("0");
        let layout = self
            .inherited
            .first()
            .filter(|r| r.has_tag_name((P, "sldLayout")))
            .and_then(|root| {
                root.descendants().find(|n| {
                    n.has_tag_name((P, "sp"))
                        && desc(*n, P, "ph")
                            .is_some_and(|p| p.attribute("idx").unwrap_or("0") == idx)
                })
            });
        let kind = ph
            .attribute("type")
            .or_else(|| {
                layout
                    .and_then(|n| desc(n, P, "ph"))
                    .and_then(|n| n.attribute("type"))
            })
            .unwrap_or("obj");
        let category = |s: &str| match s {
            "title" | "ctrTitle" => "title",
            "body" | "obj" | "subTitle" => "body",
            _ => "other",
        };
        let master = self
            .inherited
            .last()
            .filter(|r| r.has_tag_name((P, "sldMaster")))
            .and_then(|root| {
                root.descendants().find(|n| {
                    n.has_tag_name((P, "sp"))
                        && desc(*n, P, "ph").is_some_and(|p| {
                            let typ = p.attribute("type").unwrap_or("obj");
                            typ == kind
                                || (category(typ) == category(kind) && category(kind) != "other")
                        })
                })
            });
        master
            .into_iter()
            .chain(layout)
            .chain(std::iter::once(shape))
            .collect()
    }

    pub fn properties(&self, shape: X<'d, 'i>, problems: &mut Problems) -> Value {
        let mut props = Map::new();
        for layer in self.chain(shape) {
            self.visual(layer, &mut props, problems);
        }
        if let Some(body) = child(shape, P, "txBody")
            && !plain(body).trim().is_empty()
        {
            let paragraph = body.children().find(|n| n.has_tag_name((A, "p")));
            props.extend(self.text_properties(shape, paragraph, None, problems));
        } else if let Some(table) = desc(shape, A, "tbl") {
            props.extend(self.standalone_text(table, problems));
            if table.descendants().any(|n| {
                n.has_tag_name((A, "schemeClr"))
                    || n.has_tag_name((A, "alpha")) && n.attribute("val") != Some("100000")
            }) {
                problems.insert("table cell theme/alpha styling is not reconstructed".into());
            }
        }
        Value::Object(props)
    }

    pub fn standalone_text(&self, root: X<'_, '_>, problems: &mut Problems) -> Map<String, Value> {
        let mut props = Map::new();
        if let Some(ppr) = desc(root, A, "pPr") {
            self.paragraph(ppr, false, &mut props, problems);
        }
        if let Some(run) = desc(root, A, "rPr").or_else(|| desc(root, A, "defRPr")) {
            self.run(run, false, &mut props, problems);
        }
        props
    }

    pub fn text_properties(
        &self,
        shape: X<'d, 'i>,
        paragraph: Option<X<'d, 'i>>,
        run: Option<X<'d, 'i>>,
        problems: &mut Problems,
    ) -> Map<String, Value> {
        let ppr = paragraph.and_then(|p| child(p, A, "pPr"));
        let level = ppr
            .and_then(|p| p.attribute("lvl"))
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0)
            .min(8);
        let chain = self.chain(shape);
        let kind = chain
            .iter()
            .rev()
            .find_map(|n| desc(*n, P, "ph").and_then(|p| p.attribute("type")))
            .unwrap_or("obj");
        let category = if ["title", "ctrTitle"].contains(&kind) {
            "titleStyle"
        } else if ["body", "obj", "subTitle"].contains(&kind) && desc(shape, P, "ph").is_some() {
            "bodyStyle"
        } else {
            "otherStyle"
        };
        let content = run
            .map(|run| {
                run.descendants()
                    .filter(|n| n.has_tag_name((A, "t")))
                    .filter_map(|n| n.text())
                    .collect::<String>()
            })
            .or_else(|| paragraph.map(plain))
            .unwrap_or_default();
        let cjk = content
            .chars()
            .any(|c| matches!(c as u32,0x2e80..=0x9fff|0xac00..=0xd7af|0x20000..=0x323af));
        let mut props = Map::new();
        if let Some(style) = child(self.presentation, P, "defaultTextStyle") {
            self.list_style(style, level, cjk, &mut props, problems);
        }
        if let Some(style) = self
            .inherited
            .last()
            .and_then(|m| child(*m, P, "txStyles"))
            .and_then(|s| child(s, P, category))
        {
            self.list_style(style, level, cjk, &mut props, problems);
        }
        for layer in chain {
            if let Some(reference) = child(layer, P, "style").and_then(|s| child(s, A, "fontRef")) {
                if let Some(index) = reference.attribute("idx")
                    && ["major", "minor"].contains(&index)
                    && let Some(family) = self.theme.font(
                        if index == "major" {
                            if cjk { "+mj-ea" } else { "+mj-lt" }
                        } else if cjk {
                            "+mn-ea"
                        } else {
                            "+mn-lt"
                        },
                        cjk,
                        problems,
                    )
                {
                    props.insert("family".into(), family.into());
                }
                if reference.children().any(|n| n.is_element())
                    && let Some(color) = self.theme.color_child(reference, None, problems)
                {
                    props.insert("color".into(), color_hex(color).into());
                }
            }
            if let Some(body) = child(layer, P, "txBody") {
                if let Some(list) = child(body, A, "lstStyle") {
                    self.list_style(list, level, cjk, &mut props, problems);
                }
                if layer != shape
                    && let Some(pp) = body
                        .children()
                        .find(|n| n.has_tag_name((A, "p")))
                        .and_then(|p| child(p, A, "pPr"))
                {
                    self.paragraph(pp, cjk, &mut props, problems);
                }
            }
        }
        if let Some(ppr) = ppr {
            self.paragraph(ppr, cjk, &mut props, problems);
        }
        if let Some(rpr) = run.and_then(|r| child(r, A, "rPr")) {
            self.run(rpr, cjk, &mut props, problems);
        }
        props
    }

    fn list_style(
        &self,
        style: X<'_, '_>,
        level: usize,
        cjk: bool,
        props: &mut Map<String, Value>,
        problems: &mut Problems,
    ) {
        if let Some(pp) = child(style, A, "defPPr") {
            self.paragraph(pp, cjk, props, problems);
        }
        if let Some(pp) = child(style, A, &format!("lvl{}pPr", level + 1)) {
            self.paragraph(pp, cjk, props, problems);
        }
    }
    fn paragraph(
        &self,
        ppr: X<'_, '_>,
        cjk: bool,
        props: &mut Map<String, Value>,
        problems: &mut Problems,
    ) {
        if let Some(align) = ppr.attribute("algn") {
            props.insert("align".into(), align.into());
            if !["l", "ctr", "r", "just"].contains(&align) {
                problems.insert(format!("paragraph alignment {align} is not reconstructed"));
            }
        }
        if let Some(spacing) = child(ppr, A, "lnSpc") {
            if let Some(points) = child(spacing, A, "spcPts")
                .and_then(|n| n.attribute("val"))
                .and_then(|s| s.parse::<u64>().ok())
                .filter(|v| *v > 0)
            {
                props.remove("lineHeightPct");
                props.insert("lineHeight".into(), points.into());
            } else if let Some(ratio) = child(spacing, A, "spcPct")
                .and_then(|n| percentage(n.attribute("val")))
                .filter(|v| *v > 0.0)
            {
                props.remove("lineHeight");
                props.insert("lineHeightPct".into(), ratio.into());
            }
        }
        if let Some(run) = child(ppr, A, "defRPr") {
            self.run(run, cjk, props, problems);
        }
        paragraph_limits(ppr, problems);
    }
    fn run(
        &self,
        rpr: X<'_, '_>,
        cjk: bool,
        props: &mut Map<String, Value>,
        problems: &mut Problems,
    ) {
        let font = |name| {
            child(rpr, A, name)
                .and_then(|n| n.attribute("typeface"))
                .filter(|s| !s.is_empty())
        };
        let requested = if cjk {
            font("ea").or_else(|| font("latin"))
        } else {
            font("latin").or_else(|| font("ea"))
        };
        if let Some(requested) = requested
            && let Some(family) = self.theme.font(requested, cjk, problems)
        {
            props.insert("family".into(), family.into());
        }
        if let Some(size) = rpr.attribute("sz").and_then(|s| s.parse::<u64>().ok()) {
            if size > 0 {
                props.insert("size".into(), size.into());
            } else {
                problems.insert("invalid zero text size".into());
            }
        }
        for (attr, key) in [("b", "weight"), ("i", "italic")] {
            if let Some(value) = rpr.attribute(attr) {
                props.insert(
                    key.into(),
                    if value == "1" || value == "true" {
                        "1"
                    } else {
                        "0"
                    }
                    .into(),
                );
            }
        }
        if let Some(color) = self.theme.fill(rpr, None, problems) {
            props.insert("color".into(), color_hex(color).into());
        }
        for attr in ["u", "strike", "cap", "baseline", "spc"] {
            if rpr
                .attribute(attr)
                .is_some_and(|v| !["0", "none", "noStrike"].contains(&v))
            {
                problems.insert(format!("text property {attr} is not reconstructed"));
            }
        }
    }
    pub fn default_text_limits(&self) -> Problems {
        let mut problems = Problems::new();
        if let Some(style) = child(self.presentation, P, "defaultTextStyle") {
            for p in style.children().filter(|n| n.is_element()) {
                paragraph_limits(p, &mut problems);
            }
        }
        problems
    }
}

fn paragraph_limits(ppr: X<'_, '_>, problems: &mut Problems) {
    for attr in ["marL", "marR", "indent"] {
        if ppr.attribute(attr).is_some_and(|v| v != "0") {
            problems.insert(format!("paragraph {attr} is not reconstructed"));
        }
    }
    if ppr.children().any(|n| {
        n.tag_name().namespace() == Some(A)
            && ["buChar", "buAutoNum", "buBlip"].contains(&n.tag_name().name())
    }) {
        problems.insert("paragraph bullets are not reconstructed".into());
    }
    for name in ["spcBef", "spcAft", "tabLst"] {
        if child(ppr, A, name).is_some_and(|n| {
            n.descendants()
                .any(|v| v.attribute("val").is_some_and(|v| v != "0") || v.has_tag_name((A, "tab")))
        }) {
            problems.insert(format!("paragraph {name} is not reconstructed"));
        }
    }
}
