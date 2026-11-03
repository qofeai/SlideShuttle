use crate::model::*;
use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Weight, Wrap};
use std::collections::BTreeMap;

pub(crate) struct TextEngine {
    pub system: FontSystem,
    faces: BTreeMap<cosmic_text::fontdb::ID, (String, u32)>,
}
pub(crate) struct Shaped {
    pub width: f32,
    pub height: f32,
    pub lines: usize,
    pub glyphs: Vec<PositionedGlyph>,
}
impl TextEngine {
    pub fn new(resources: &Resources) -> Result<Self> {
        if resources.fonts.is_empty() {
            return Err(invalid("at least one explicit font resource is required"));
        }
        let mut db = cosmic_text::fontdb::Database::new();
        let mut keys = BTreeMap::new();
        for font in &resources.fonts {
            if font.bytes.len() > 64 * 1024 * 1024 {
                return Err(invalid("font exceeds 64 MiB"));
            }
            let before = db.faces().map(|f| f.id).collect::<Vec<_>>();
            db.load_font_data(font.bytes.clone());
            let font_key = digest(&font.bytes);
            for f in db.faces().filter(|f| !before.contains(&f.id)) {
                keys.insert(f.id, (font_key.clone(), f.index));
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
    pub fn shape(
        &mut self,
        spans: &[Span],
        base: &TextStyle,
        width: Option<f32>,
        wrap: bool,
    ) -> Result<Shaped> {
        if base.size <= 0.0 || base.line_height <= 0.0 {
            return Err(invalid("font size and line height must be positive"));
        }
        let styles = spans
            .iter()
            .map(|s| text_style(&s.style, base))
            .collect::<Result<Vec<_>>>()?;
        // An empty object has no line box. Explicit whitespace/newlines remain
        // measurable content, so they still reserve authored line spacing.
        if spans.iter().all(|span| span.text.is_empty()) {
            return Ok(Shaped {
                width: 0.0,
                height: 0.0,
                lines: 0,
                glyphs: Vec::new(),
            });
        }
        for style in &styles {
            if !self.has_face(style) {
                return Err(invalid(format!(
                    "font face not supplied: {} weight={} italic={}",
                    style.family, style.weight, style.italic
                )));
            }
        }
        fn attr(s: &TextStyle) -> Attrs<'_> {
            Attrs::new()
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
                .metrics(Metrics::new(s.size, s.line_height))
        }
        let default = attr(base);
        let rich = spans
            .iter()
            .zip(styles.iter())
            .map(|(span, style)| (span.text.as_str(), attr(style)))
            .collect::<Vec<_>>();
        let mut buffer = Buffer::new(&mut self.system, Metrics::new(base.size, base.line_height));
        buffer.set_size(width.map(|v| v.max(0.01)), None);
        buffer.set_wrap(if wrap { Wrap::WordOrGlyph } else { Wrap::None });
        let align = match base.align.as_str() {
            "left" => cosmic_text::Align::Left,
            "center" => cosmic_text::Align::Center,
            "right" => cosmic_text::Align::Right,
            "justify" => cosmic_text::Align::Justified,
            _ => return Err(invalid("unsupported text-align")),
        };
        buffer.set_rich_text(rich, &default, Shaping::Advanced, Some(align));
        buffer.shape_until_scroll(&mut self.system, false);
        let (mut w, mut h, mut lines) = (0.0f32, 0.0f32, 0);
        let mut glyphs = Vec::new();
        for run in buffer.layout_runs() {
            lines += 1;
            w = w.max(run.line_w);
            h = h.max(run.line_top + run.line_height);
            for g in run.glyphs {
                let (key, index) = self
                    .faces
                    .get(&g.font_id)
                    .ok_or_else(|| invalid("font resolver returned an unregistered face"))?;
                let c = g.color_opt.unwrap_or(default.color_opt.unwrap());
                let text = run.text.get(g.start..g.end).unwrap_or("").to_owned();
                if g.glyph_id == 0 && !text.chars().all(char::is_whitespace) {
                    return Err(invalid(format!("font has no glyph for {text:?}")));
                }
                glyphs.push(PositionedGlyph {
                    font_key: key.clone(),
                    font_index: *index,
                    glyph_id: g.glyph_id,
                    size: g.font_size,
                    weight: g.font_weight.0,
                    x: g.x + g.x_offset * g.font_size,
                    y: run.line_y + g.y - g.y_offset * g.font_size,
                    advance: g.w,
                    text,
                    color: Color(c.r(), c.g(), c.b(), c.a()),
                });
            }
        }
        if lines == 0 {
            h = base.line_height;
            lines = 1;
        }
        Ok(Shaped {
            width: w,
            height: h,
            lines,
            glyphs,
        })
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
    if let Some(v) = css.get("text-align") {
        if !["left", "center", "right", "justify"].contains(&v.as_str()) {
            return Err(invalid("unsupported text-align"));
        }
        s.align = v.clone();
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
