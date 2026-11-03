use crate::model::*;
use cosmic_text::{
    Attrs, Buffer, Family, FontSystem, Metrics, Shaping, UnderlineStyle, Weight, Wrap,
};
use std::collections::BTreeMap;

pub(crate) struct TextEngine {
    pub system: FontSystem,
    faces: BTreeMap<cosmic_text::fontdb::ID, (usize, u32)>,
}
#[derive(Default)]
pub(crate) struct Shaped {
    pub width: f32,
    pub height: f32,
    pub lines: usize,
    pub glyphs: Vec<PositionedGlyph>,
    pub marks: Vec<TextMark>,
    /// Baseline of the first line; aligns list markers.
    pub first_baseline: f32,
}
impl Shaped {
    pub fn offset(&mut self, dx: f32, dy: f32) {
        for g in &mut self.glyphs {
            g.x += dx;
            g.y += dy;
        }
        for m in &mut self.marks {
            m.rect.x += dx;
            m.rect.y += dy;
        }
    }
}
/// Hanging indent per list level, as a multiple of the font size.
const LIST_STEP: f32 = 1.5;
/// Superscript/subscript glyphs are this fraction of the span's font size.
const SCRIPT_SCALE: f32 = 0.65;
fn marker(list: ListMarker) -> String {
    if list.ordered {
        format!("{}.", list.number)
    } else {
        ["•", "–"][usize::from(list.level % 2)].into()
    }
}
impl TextEngine {
    pub fn new(resources: &Resources) -> Result<Self> {
        if resources.fonts.is_empty() {
            return Err(invalid("at least one explicit font resource is required"));
        }
        let mut db = cosmic_text::fontdb::Database::new();
        let mut keys = BTreeMap::new();
        for (font_number, font) in resources.fonts.iter().enumerate() {
            if font.bytes.len() > 64 * 1024 * 1024 {
                return Err(invalid("font exceeds 64 MiB"));
            }
            let before = db.faces().map(|f| f.id).collect::<Vec<_>>();
            db.load_font_data(font.bytes.clone());
            let mut families = Vec::new();
            for f in db.faces().filter(|f| !before.contains(&f.id)) {
                keys.insert(f.id, (font_number, f.index));
                families.extend(f.families.iter().map(|n| n.0.as_str()));
            }
            if let Some(family) = &font.family
                && !families.contains(&family.as_str())
            {
                families.sort_unstable();
                families.dedup();
                return Err(invalid(format!(
                    "declared font family {family} is not in its file; the file provides: {}",
                    families.join(", ")
                )));
            }
        }
        if keys.is_empty() {
            return Err(invalid("no valid font faces in supplied resources"));
        }
        let first_family = db
            .faces()
            .next()
            .and_then(|f| f.families.first())
            .map(|f| f.0.clone())
            .unwrap();
        db.set_sans_serif_family(&first_family);
        db.set_serif_family(&first_family);
        db.set_monospace_family(&first_family);
        Ok(Self {
            system: FontSystem::new_with_locale_and_db("zh-CN".into(), db),
            faces: keys,
        })
    }
    /// Shape paragraphs into one block: list items get a hanging indent and a marker,
    /// `margin-top` adds space before a paragraph and `text-align` overrides per paragraph.
    pub fn shape(
        &mut self,
        paragraphs: &[Paragraph],
        base: &TextStyle,
        width: Option<f32>,
        wrap: bool,
    ) -> Result<Shaped> {
        if base.size <= 0.0 || base.line_height <= 0.0 {
            return Err(invalid("font size and line height must be positive"));
        }
        // An empty object has no line box. Explicit whitespace/newlines remain
        // measurable content, so they still reserve authored line spacing.
        if paragraphs
            .iter()
            .all(|p| p.list.is_none() && p.spans.iter().all(|span| span.text.is_empty()))
        {
            return Ok(Shaped::default());
        }
        let mut out = Shaped::default();
        let mut y = 0.0;
        for (i, p) in paragraphs.iter().enumerate() {
            let mut style = base.clone();
            if let Some(align) = p.style.get("text-align") {
                style.align = keyword(align, "text-align")?;
            }
            if let Some(margin) = p.style.get("margin-top") {
                let margin = length_px(margin)?;
                if !(0.0..=4096.0).contains(&margin) {
                    return Err(invalid("paragraph margin-top must be in [0, 4096]px"));
                }
                y += margin;
            }
            // Markers and indents follow the item's own text, its first span.
            let first = match p.spans.first() {
                Some(span) => text_style(&span.style, &style)?,
                None => style.clone(),
            };
            let step = LIST_STEP * first.size;
            let indent = p.list.map_or(0.0, |l| f32::from(l.level + 1) * step);
            let mut part = self.shape_spans(
                &p.spans,
                &style,
                width.map(|w| (w - indent).max(0.01)),
                wrap,
            )?;
            if part.lines == 0 {
                // An empty paragraph among others still occupies its line.
                part.height = style.line_height;
                part.lines = 1;
                part.first_baseline = style.line_height * 0.8;
            }
            if let Some(list) = p.list {
                let marker_style = TextStyle {
                    align: Align::Left,
                    underline: false,
                    strike: false,
                    ..first
                };
                let marker = Paragraph::plain(marker(list)).spans;
                let mut mark = self.shape_spans(&marker, &marker_style, None, false)?;
                mark.offset(indent - step, y + part.first_baseline - mark.first_baseline);
                out.glyphs.append(&mut mark.glyphs);
            }
            if i == 0 {
                out.first_baseline = y + part.first_baseline;
            }
            part.offset(indent, y);
            out.width = out.width.max(indent + part.width);
            out.lines += part.lines;
            out.glyphs.append(&mut part.glyphs);
            out.marks.append(&mut part.marks);
            y += part.height;
        }
        out.height = y;
        Ok(out)
    }
    fn shape_spans(
        &mut self,
        spans: &[Span],
        base: &TextStyle,
        width: Option<f32>,
        wrap: bool,
    ) -> Result<Shaped> {
        if spans.iter().all(|span| span.text.is_empty()) {
            return Ok(Shaped::default());
        }
        struct Look {
            style: TextStyle,
            /// Baseline shift as a multiple of the span's font size.
            shift: f32,
            highlight: Option<Color>,
        }
        let looks = spans
            .iter()
            .map(|span| {
                let style = text_style(&span.style, base)?;
                let shift = match span.style.get("vertical-align").map(String::as_str) {
                    None | Some("baseline") => 0.0,
                    Some("super") => -0.35,
                    Some("sub") => 0.15,
                    Some(other) => {
                        return Err(invalid(format!(
                            "Span vertical-align is baseline, super or sub, not {other}"
                        )));
                    }
                };
                let highlight = span
                    .style
                    .get("background")
                    .map(|v| Color::parse(v))
                    .transpose()?;
                Ok(Look {
                    style,
                    shift,
                    highlight,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        for look in &looks {
            if !self.has_face(&look.style) {
                return Err(invalid(format!(
                    "font face not supplied: {} weight={} italic={}",
                    look.style.family, look.style.weight, look.style.italic
                )));
            }
        }
        fn attr(look: &Look, index: usize) -> Attrs<'_> {
            let s = &look.style;
            let size = if look.shift == 0.0 {
                s.size
            } else {
                s.size * SCRIPT_SCALE
            };
            let mut attrs = Attrs::new()
                .family(Family::Name(&s.family))
                .weight(Weight(s.weight))
                .style(if s.italic {
                    cosmic_text::Style::Italic
                } else {
                    cosmic_text::Style::Normal
                })
                .color(cosmic_text::Color::rgba(
                    s.color.0, s.color.1, s.color.2, s.color.3,
                ))
                .metrics(Metrics::new(size, s.line_height))
                .metadata(index);
            if s.letter_spacing != 0.0 {
                attrs = attrs.letter_spacing(s.letter_spacing / size);
            }
            if s.underline {
                attrs = attrs.underline(UnderlineStyle::Single);
            }
            if s.strike {
                attrs = attrs.strikethrough();
            }
            attrs
        }
        let base_look = Look {
            style: base.clone(),
            shift: 0.0,
            highlight: None,
        };
        let default = attr(&base_look, usize::MAX);
        let rich = spans
            .iter()
            .zip(looks.iter())
            .enumerate()
            .map(|(i, (span, look))| (span.text.as_str(), attr(look, i)))
            .collect::<Vec<_>>();
        let mut buffer = Buffer::new(&mut self.system, Metrics::new(base.size, base.line_height));
        buffer.set_size(width.map(|v| v.max(0.01)), None);
        buffer.set_wrap(if wrap { Wrap::WordOrGlyph } else { Wrap::None });
        let align = match base.align {
            Align::Left => cosmic_text::Align::Left,
            Align::Center => cosmic_text::Align::Center,
            Align::Right => cosmic_text::Align::Right,
            Align::Justify => cosmic_text::Align::Justified,
        };
        buffer.set_rich_text(rich, &default, Shaping::Advanced, Some(align));
        buffer.shape_until_scroll(&mut self.system, false);
        let mut out = Shaped::default();
        for run in buffer.layout_runs() {
            if out.lines == 0 {
                out.first_baseline = run.line_y;
            }
            out.lines += 1;
            out.width = out.width.max(run.line_w);
            out.height = out.height.max(run.line_top + run.line_height);
            // Highlights and link areas cover consecutive glyphs of one span.
            let mut start = 0;
            while start < run.glyphs.len() {
                let span = run.glyphs[start].metadata;
                let end = run.glyphs[start..]
                    .iter()
                    .position(|g| g.metadata != span)
                    .map_or(run.glyphs.len(), |n| start + n);
                let (x0, x1) = run.glyphs[start..end]
                    .iter()
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), g| {
                        (a.min(g.x), b.max(g.x + g.w))
                    });
                let rect = Rect {
                    x: x0,
                    y: run.line_top,
                    width: (x1 - x0).max(0.0),
                    height: run.line_height,
                };
                if let Some(color) = looks.get(span).and_then(|l| l.highlight) {
                    out.marks.push(TextMark {
                        rect,
                        color,
                        kind: MarkKind::Highlight,
                    });
                }
                if let Some(href) = spans.get(span).and_then(|s| s.href.clone()) {
                    out.marks.push(TextMark {
                        rect,
                        color: Color(0, 0, 0, 0),
                        kind: MarkKind::Link(href),
                    });
                }
                start = end;
            }
            for deco in run.decorations {
                let glyphs = &run.glyphs[deco.glyph_range.clone()];
                let Some(first) = glyphs.first() else {
                    continue;
                };
                let (x0, x1) = glyphs
                    .iter()
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), g| {
                        (a.min(g.x), b.max(g.x + g.w))
                    });
                let c = first.color_opt.or(default.color_opt).unwrap();
                let color = Color(c.r(), c.g(), c.b(), c.a());
                let td = &deco.data.text_decoration;
                for (on, metrics) in [
                    (
                        td.underline != UnderlineStyle::None,
                        deco.data.underline_metrics,
                    ),
                    (td.strikethrough, deco.data.strikethrough_metrics),
                ] {
                    if on {
                        out.marks.push(TextMark {
                            rect: Rect {
                                x: x0,
                                y: run.line_y - metrics.offset * deco.font_size,
                                width: (x1 - x0).max(0.0),
                                height: (metrics.thickness * deco.font_size).max(1.0),
                            },
                            color,
                            kind: MarkKind::Line,
                        });
                    }
                }
            }
            for g in run.glyphs {
                let (font, index) = self
                    .faces
                    .get(&g.font_id)
                    .ok_or_else(|| invalid("font resolver returned an unregistered face"))?;
                let c = g.color_opt.unwrap_or(default.color_opt.unwrap());
                let text = run.text.get(g.start..g.end).unwrap_or("").to_owned();
                if g.glyph_id == 0 && !text.chars().all(char::is_whitespace) {
                    return Err(invalid(format!("font has no glyph for {text:?}")));
                }
                let shift = looks
                    .get(g.metadata)
                    .map_or(0.0, |l| l.shift * l.style.size);
                out.glyphs.push(PositionedGlyph {
                    font: *font,
                    font_index: *index,
                    glyph_id: g.glyph_id,
                    size: g.font_size,
                    weight: g.font_weight.0,
                    x: g.x + g.x_offset * g.font_size,
                    y: run.line_y + g.y - g.y_offset * g.font_size + shift,
                    advance: g.w,
                    text,
                    color: Color(c.r(), c.g(), c.b(), c.a()),
                });
            }
        }
        if out.lines == 0 {
            out.height = base.line_height;
            out.lines = 1;
        }
        Ok(out)
    }
    pub fn has_face(&self, style: &TextStyle) -> bool {
        self.system.db().faces().any(|f| {
            f.families.iter().any(|n| n.0 == style.family)
                && f.weight.0 == style.weight
                && f.style
                    == if style.italic {
                        cosmic_text::fontdb::Style::Italic
                    } else {
                        cosmic_text::fontdb::Style::Normal
                    }
        })
    }
}

pub(crate) fn text_style(css: &Css, parent: &TextStyle) -> Result<TextStyle> {
    let mut s = parent.clone();
    if let Some(v) = css.get("font-family") {
        s.family = v.trim_matches(['\'', '"']).to_owned();
    }
    if let Some(v) = css.get("font-size") {
        s.size = length_px(v)?;
    }
    if let Some(v) = css.get("line-height") {
        s.line_height = if let Ok(mult) = v.parse::<f32>() {
            finite(mult, "line-height")? * s.size
        } else {
            length_px(v)?
        };
    } else if css.contains_key("font-size") {
        s.line_height = s.size * (parent.line_height / parent.size);
    }
    if let Some(v) = css.get("font-weight") {
        s.weight = match v.as_str() {
            "normal" => 400,
            "bold" => 700,
            _ => v.parse().map_err(|_| invalid("invalid font-weight"))?,
        };
        if ![400, 700].contains(&s.weight) {
            return Err(invalid("font-weight supports 400 or 700"));
        }
    }
    if let Some(v) = css.get("font-style") {
        s.italic = match v.as_str() {
            "normal" => false,
            "italic" => true,
            _ => return Err(invalid("font-style supports normal or italic")),
        };
    }
    if let Some(v) = css.get("color") {
        s.color = Color::parse(v)?;
    }
    if let Some(v) = css.get("letter-spacing") {
        s.letter_spacing = if v == "normal" {
            0.0
        } else if let Some(em) = v.strip_suffix("em") {
            finite(
                em.trim()
                    .parse::<f32>()
                    .map_err(|_| invalid("invalid letter-spacing"))?,
                "letter-spacing",
            )? * s.size
        } else {
            length_px(v)?
        };
        if s.letter_spacing.abs() > s.size * 2.0 {
            return Err(invalid("letter-spacing must stay within ±2em"));
        }
    }
    if let Some(v) = css.get("text-decoration") {
        s.underline = false;
        s.strike = false;
        for part in v.split_whitespace() {
            match part {
                "none" => {}
                "underline" => s.underline = true,
                "line-through" => s.strike = true,
                other => {
                    return Err(invalid(format!(
                        "text-decoration supports none, underline and line-through, not {other}"
                    )));
                }
            }
        }
    }
    if let Some(v) = css.get("text-align") {
        s.align = keyword(v, "text-align")?;
    }
    if !s.size.is_finite()
        || !s.line_height.is_finite()
        || s.size <= 0.0
        || s.size > 1024.0
        || s.line_height <= 0.0
        || s.line_height > 4096.0
    {
        return Err(invalid("text metrics out of bounds"));
    }
    Ok(s)
}
