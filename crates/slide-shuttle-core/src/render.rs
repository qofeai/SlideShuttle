//! Native drawing of the resolved scene. Neither backend performs text layout.
use crate::model::*;
use std::collections::{BTreeMap, btree_map::Entry};

#[derive(Clone, Copy)]
pub(crate) enum Command {
    M(f32, f32),
    L(f32, f32),
    C(f32, f32, f32, f32, f32, f32),
    Z,
}

pub(crate) fn item_path(item: &Item) -> Result<Vec<Command>> {
    let r = item.rect;
    let x = r.x;
    let y = r.y;
    let w = r.width;
    let h = r.height;
    if let Draw::Path { data, view_box } = &item.kind {
        let mut commands = Vec::new();
        for segment in svgtypes::PathParser::from(data.as_str()) {
            use svgtypes::PathSegment::*;
            let p = segment.map_err(|e| invalid(e.to_string()))?;
            let xx = |v: f64| finite(x + v as f32 * w / view_box[0], "path coordinate");
            let yy = |v: f64| finite(y + v as f32 * h / view_box[1], "path coordinate");
            commands.push(match p {
                MoveTo { abs: true, x, y } => Command::M(xx(x)?, yy(y)?),
                LineTo { abs: true, x, y } => Command::L(xx(x)?, yy(y)?),
                CurveTo {
                    abs: true,
                    x1,
                    y1,
                    x2,
                    y2,
                    x,
                    y,
                } => Command::C(xx(x1)?, yy(y1)?, xx(x2)?, yy(y2)?, xx(x)?, yy(y)?),
                ClosePath { .. } => Command::Z,
                _ => return Err(invalid("unsupported path command")),
            });
        }
        return Ok(commands);
    }
    if matches!(item.kind, Draw::Shape(ShapeKind::Line)) {
        return Ok(vec![Command::M(x, y), Command::L(x + w, y + h)]);
    }
    if matches!(item.kind, Draw::Shape(ShapeKind::Ellipse)) {
        let (a, b, cx, cy) = (w / 2.0, h / 2.0, x + w / 2.0, y + h / 2.0);
        let k = 0.5522848;
        return Ok(vec![
            Command::M(cx + a, cy),
            Command::C(cx + a, cy + b * k, cx + a * k, cy + b, cx, cy + b),
            Command::C(cx - a * k, cy + b, cx - a, cy + b * k, cx - a, cy),
            Command::C(cx - a, cy - b * k, cx - a * k, cy - b, cx, cy - b),
            Command::C(cx + a * k, cy - b, cx + a, cy - b * k, cx + a, cy),
            Command::Z,
        ]);
    }
    let t = item.radius.min(w / 2.0).min(h / 2.0).max(0.0);
    let k = 0.5522848 * t;
    Ok(vec![
        Command::M(x + t, y),
        Command::L(x + w - t, y),
        Command::C(x + w - t + k, y, x + w, y + t - k, x + w, y + t),
        Command::L(x + w, y + h - t),
        Command::C(x + w, y + h - t + k, x + w - t + k, y + h, x + w - t, y + h),
        Command::L(x + t, y + h),
        Command::C(x + t - k, y + h, x, y + h - t + k, x, y + h - t),
        Command::L(x, y + t),
        Command::C(x, y + t - k, x + t - k, y, x + t, y),
        Command::Z,
    ])
}
fn skia_path(commands: &[Command]) -> Option<tiny_skia::Path> {
    let mut b = tiny_skia::PathBuilder::new();
    for c in commands {
        match *c {
            Command::M(x, y) => b.move_to(x, y),
            Command::L(x, y) => b.line_to(x, y),
            Command::C(a, b1, c, d, e, f) => b.cubic_to(a, b1, c, d, e, f),
            Command::Z => b.close(),
        }
    }
    b.finish()
}
fn pdf_path(commands: &[Command]) -> Option<krilla::geom::Path> {
    let mut b = krilla::geom::PathBuilder::new();
    for c in commands {
        match *c {
            Command::M(x, y) => b.move_to(x, y),
            Command::L(x, y) => b.line_to(x, y),
            Command::C(a, b1, c, d, e, f) => b.cubic_to(a, b1, c, d, e, f),
            Command::Z => b.close(),
        }
    }
    b.finish()
}
fn font_bytes<'r>(resources: &'r Resources, glyph: &PositionedGlyph) -> Result<&'r [u8]> {
    resources
        .fonts
        .get(glyph.font)
        .map(|f| f.bytes.as_slice())
        .ok_or_else(|| invalid("missing glyph font bytes"))
}
type Faces<'r> = BTreeMap<(usize, u32), ttf_parser::Face<'r>>;
fn ensure(scene: &Scene) -> Result<()> {
    if scene.has_errors() {
        Err(invalid(
            "scene has error diagnostics; run check and correct them before rendering/export",
        ))
    } else {
        Ok(())
    }
}

pub fn render_png(
    scene: &Scene,
    page: usize,
    resources: &Resources,
    scale: f32,
) -> Result<Vec<u8>> {
    ensure(scene)?;
    if !scale.is_finite() || scale <= 0.0 || scale > 4.0 {
        return Err(invalid("render scale must be in (0,4]"));
    }
    let page = scene
        .pages
        .get(page)
        .ok_or_else(|| invalid("page index out of range"))?;
    let (w, h) = (
        (scene.width * scale).ceil() as u32,
        (scene.height * scale).ceil() as u32,
    );
    if u64::from(w) * u64::from(h) > 64_000_000 {
        return Err(invalid("render exceeds 64 megapixels"));
    }
    let mut pixmap =
        tiny_skia::Pixmap::new(w, h).ok_or_else(|| invalid("cannot allocate render surface"))?;
    pixmap.fill(tiny_skia::Color::from_rgba8(
        page.background.0,
        page.background.1,
        page.background.2,
        page.background.3,
    ));
    let mut faces = Faces::new();
    for item in &page.items {
        paint_png(item, &mut pixmap, resources, &mut faces, scale)?;
    }
    pixmap.encode_png().map_err(|e| invalid(e.to_string()))
}

struct Outline<'a> {
    builder: &'a mut tiny_skia::PathBuilder,
    x: f32,
    y: f32,
    scale: f32,
}
impl ttf_parser::OutlineBuilder for Outline<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.builder
            .move_to(self.x + x * self.scale, self.y - y * self.scale);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.builder
            .line_to(self.x + x * self.scale, self.y - y * self.scale);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.builder.quad_to(
            self.x + x1 * self.scale,
            self.y - y1 * self.scale,
            self.x + x * self.scale,
            self.y - y * self.scale,
        );
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.builder.cubic_to(
            self.x + x1 * self.scale,
            self.y - y1 * self.scale,
            self.x + x2 * self.scale,
            self.y - y2 * self.scale,
            self.x + x * self.scale,
            self.y - y * self.scale,
        );
    }
    fn close(&mut self) {
        self.builder.close();
    }
}
fn paint(color: Color, opacity: f32) -> tiny_skia::Paint<'static> {
    let mut p = tiny_skia::Paint::default();
    p.set_color_rgba8(
        color.0,
        color.1,
        color.2,
        (color.3 as f32 * opacity).round() as u8,
    );
    p.anti_alias = true;
    p
}
fn paint_png<'r>(
    item: &Item,
    pixmap: &mut tiny_skia::Pixmap,
    resources: &'r Resources,
    faces: &mut Faces<'r>,
    scale: f32,
) -> Result<()> {
    let transform = tiny_skia::Transform::from_scale(scale, scale);
    if (item.fill.3 > 0 || item.stroke_width > 0.0)
        && let Some(path) = skia_path(&item_path(item)?)
    {
        if item.fill.3 > 0 {
            let mut fill = paint(item.fill, item.opacity);
            if let Some(g) = item.gradient {
                let r = item.rect;
                fill.shader = tiny_skia::LinearGradient::new(
                    tiny_skia::Point::from_xy(r.x, r.y),
                    tiny_skia::Point::from_xy(
                        if g.vertical { r.x } else { r.right() },
                        if g.vertical { r.bottom() } else { r.y },
                    ),
                    vec![
                        tiny_skia::GradientStop::new(
                            0.0,
                            tiny_skia::Color::from_rgba8(
                                g.start.0,
                                g.start.1,
                                g.start.2,
                                (item.opacity * 255.0).round() as u8,
                            ),
                        ),
                        tiny_skia::GradientStop::new(
                            1.0,
                            tiny_skia::Color::from_rgba8(
                                g.end.0,
                                g.end.1,
                                g.end.2,
                                (item.opacity * 255.0).round() as u8,
                            ),
                        ),
                    ],
                    tiny_skia::SpreadMode::Pad,
                    tiny_skia::Transform::identity(),
                )
                .ok_or_else(|| invalid("empty gradient rectangle"))?;
            }
            pixmap.fill_path(&path, &fill, tiny_skia::FillRule::Winding, transform, None);
        }
        if item.stroke_width > 0.0 {
            pixmap.stroke_path(
                &path,
                &paint(item.stroke, item.opacity),
                &tiny_skia::Stroke {
                    width: item.stroke_width,
                    ..Default::default()
                },
                transform,
                None,
            );
        }
    }
    match &item.kind {
        Draw::Text { glyphs, .. } => {
            for g in glyphs {
                let face = match faces.entry((g.font, g.font_index)) {
                    Entry::Occupied(e) => e.into_mut(),
                    Entry::Vacant(e) => e.insert(
                        ttf_parser::Face::parse(font_bytes(resources, g)?, g.font_index)
                            .map_err(|e| invalid(format!("font parse: {e}")))?,
                    ),
                };
                let mut builder = tiny_skia::PathBuilder::new();
                face.outline_glyph(
                    ttf_parser::GlyphId(g.glyph_id),
                    &mut Outline {
                        builder: &mut builder,
                        x: g.x,
                        y: g.y,
                        scale: g.size / face.units_per_em() as f32,
                    },
                );
                if let Some(path) = builder.finish() {
                    pixmap.fill_path(
                        &path,
                        &paint(g.color, item.opacity),
                        tiny_skia::FillRule::Winding,
                        transform,
                        None,
                    );
                }
            }
        }
        Draw::Image { src, fit } => {
            let bytes = resources
                .images
                .get(src)
                .ok_or_else(|| invalid(format!("missing image {src}")))?;
            let image = decode_image(bytes)?;
            let (w, h) = image.dimensions();
            let dest = image_rect(item.rect, w as f32, h as f32, fit);
            let dw = (dest.width * scale).ceil().max(1.0) as u32;
            let dh = (dest.height * scale).ceil().max(1.0) as u32;
            if u64::from(dw) * u64::from(dh) > 64_000_000 {
                return Err(invalid("image render exceeds pixel budget"));
            }
            let resized =
                image::imageops::resize(&image, dw, dh, image::imageops::FilterType::Triangle);
            for (ox, oy, pixel) in resized.enumerate_pixels() {
                let px = (dest.x * scale).round() as i32 + ox as i32;
                let py = (dest.y * scale).round() as i32 + oy as i32;
                if px < 0 || py < 0 || px >= pixmap.width() as i32 || py >= pixmap.height() as i32 {
                    continue;
                }
                let fx = px as f32 / scale;
                let fy = py as f32 / scale;
                if fx < item.rect.x
                    || fy < item.rect.y
                    || fx >= item.rect.right()
                    || fy >= item.rect.bottom()
                {
                    continue;
                }
                let alpha = (pixel[3] as f32 * item.opacity).round() as u8;
                let src = tiny_skia::ColorU8::from_rgba(pixel[0], pixel[1], pixel[2], alpha)
                    .premultiply();
                let index = py as usize * pixmap.width() as usize + px as usize;
                let dst = pixmap.pixels()[index];
                let inv = 255 - u32::from(alpha);
                let blend =
                    |a: u8, b: u8| (u32::from(a) + (u32::from(b) * inv + 127) / 255).min(255) as u8;
                pixmap.pixels_mut()[index] = tiny_skia::PremultipliedColorU8::from_rgba(
                    blend(src.red(), dst.red()),
                    blend(src.green(), dst.green()),
                    blend(src.blue(), dst.blue()),
                    blend(src.alpha(), dst.alpha()),
                )
                .unwrap();
            }
        }
        Draw::Table { cells, .. } => {
            for cell in cells {
                paint_png(cell, pixmap, resources, faces, scale)?;
            }
        }
        Draw::Chart { primitives, .. } => {
            for p in primitives {
                paint_png(p, pixmap, resources, faces, scale)?;
            }
        }
        _ => {}
    }
    Ok(())
}
pub(crate) fn image_reader(bytes: &[u8]) -> Result<image::ImageReader<std::io::Cursor<&[u8]>>> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| invalid(e.to_string()))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader)
}
fn image_error(e: image::ImageError) -> Error {
    invalid(format!("invalid or excessive image: {e}"))
}
pub(crate) fn decode_image(bytes: &[u8]) -> Result<image::RgbaImage> {
    image_reader(bytes)?
        .decode()
        .map(|i| i.to_rgba8())
        .map_err(image_error)
}
/// Reads only the header; layout needs intrinsic size, not pixels.
pub(crate) fn image_size(bytes: &[u8]) -> Result<(u32, u32)> {
    image_reader(bytes)?.into_dimensions().map_err(image_error)
}
pub(crate) fn image_rect(rect: Rect, w: f32, h: f32, fit: &str) -> Rect {
    if fit == "fill" {
        return rect;
    }
    let ratio = if fit == "cover" {
        (rect.width / w).max(rect.height / h)
    } else {
        (rect.width / w).min(rect.height / h)
    };
    let (width, height) = (w * ratio, h * ratio);
    Rect {
        x: rect.x + (rect.width - width) / 2.0,
        y: rect.y + (rect.height - height) / 2.0,
        width,
        height,
    }
}

pub fn render_pdf(scene: &Scene, resources: &Resources) -> Result<Vec<u8>> {
    ensure(scene)?;
    let mut doc = krilla::Document::new();
    let mut font_cache = BTreeMap::new();
    for page in &scene.pages {
        let settings = krilla::page::PageSettings::from_wh(scene.width * 0.75, scene.height * 0.75)
            .ok_or_else(|| invalid("invalid PDF page dimensions"))?;
        let mut p = doc.start_page_with(settings);
        let mut surface = p.surface();
        surface.push_transform(&krilla::geom::Transform::from_scale(0.75, 0.75));
        let bg = Item {
            id: "background".into(),
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: scene.width,
                height: scene.height,
            },
            fill: page.background,
            gradient: None,
            stroke: Color::BLACK,
            stroke_width: 0.0,
            radius: 0.0,
            opacity: 1.0,
            group: None,
            kind: Draw::Box,
            text_style: TextStyle::default(),
            source: None,
        };
        paint_pdf(&bg, &mut surface, resources, &mut font_cache)?;
        for item in &page.items {
            paint_pdf(item, &mut surface, resources, &mut font_cache)?;
        }
        surface.pop();
        surface.finish();
        p.finish();
    }
    doc.finish()
        .map_err(|e| invalid(format!("PDF serialization: {e:?}")))
}
fn pdf_fill(c: Color, opacity: f32) -> krilla::paint::Fill {
    krilla::paint::Fill {
        paint: krilla::color::rgb::Color::new(c.0, c.1, c.2).into(),
        opacity: krilla::num::NormalizedF32::new(c.3 as f32 / 255.0 * opacity).unwrap(),
        rule: Default::default(),
    }
}
fn paint_pdf(
    item: &Item,
    surface: &mut krilla::surface::Surface,
    resources: &Resources,
    fonts: &mut BTreeMap<(usize, u32), krilla::text::Font>,
) -> Result<()> {
    surface.set_fill(if item.fill.3 > 0 {
        Some(pdf_fill(item.fill, item.opacity))
    } else {
        None
    });
    if let Some(g) = item.gradient {
        let r = item.rect;
        let gradient = krilla::paint::LinearGradient {
            x1: r.x,
            y1: r.y,
            x2: if g.vertical { r.x } else { r.right() },
            y2: if g.vertical { r.bottom() } else { r.y },
            transform: Default::default(),
            spread_method: Default::default(),
            anti_alias: true,
            stops: [(0.0, g.start), (1.0, g.end)]
                .into_iter()
                .map(|(o, c)| krilla::paint::Stop {
                    offset: krilla::num::NormalizedF32::new(o).unwrap(),
                    color: krilla::color::rgb::Color::new(c.0, c.1, c.2).into(),
                    opacity: krilla::num::NormalizedF32::new(item.opacity).unwrap(),
                })
                .collect(),
        };
        surface.set_fill(Some(krilla::paint::Fill {
            paint: gradient.into(),
            opacity: krilla::num::NormalizedF32::ONE,
            rule: Default::default(),
        }));
    }
    surface.set_stroke(if item.stroke_width > 0.0 {
        Some(krilla::paint::Stroke {
            paint: krilla::color::rgb::Color::new(item.stroke.0, item.stroke.1, item.stroke.2)
                .into(),
            width: item.stroke_width,
            opacity: krilla::num::NormalizedF32::new(item.stroke.3 as f32 / 255.0 * item.opacity)
                .unwrap(),
            ..Default::default()
        })
    } else {
        None
    });
    if (item.fill.3 > 0 || item.stroke_width > 0.0)
        && let Some(path) = pdf_path(&item_path(item)?)
    {
        surface.draw_path(&path);
    }
    match &item.kind {
        Draw::Text { glyphs, .. } => {
            surface.set_stroke(None);
            for g in glyphs {
                let font = match fonts.entry((g.font, g.font_index)) {
                    Entry::Occupied(e) => e.get().clone(),
                    Entry::Vacant(e) => e
                        .insert(
                            krilla::text::Font::new(
                                font_bytes(resources, g)?.to_vec().into(),
                                g.font_index,
                            )
                            .ok_or_else(|| invalid("PDF font is unsupported"))?,
                        )
                        .clone(),
                };
                surface.set_fill(Some(pdf_fill(g.color, item.opacity)));
                let glyph = krilla::text::KrillaGlyph {
                    glyph_id: krilla::text::GlyphId::new(u32::from(g.glyph_id)),
                    text_range: 0..g.text.len(),
                    x_advance: g.advance / g.size,
                    x_offset: 0.0,
                    y_offset: 0.0,
                    y_advance: 0.0,
                    location: None,
                };
                surface.draw_glyphs(
                    krilla::geom::Point::from_xy(g.x, g.y),
                    &[glyph],
                    font,
                    &g.text,
                    g.size,
                    false,
                );
            }
        }
        Draw::Image { src, fit } => {
            let data = resources
                .images
                .get(src)
                .ok_or_else(|| invalid("missing image"))?;
            let decoded = decode_image(data)?;
            let dest = image_rect(
                item.rect,
                decoded.width() as f32,
                decoded.height() as f32,
                fit,
            );
            if let Some(clip) = pdf_path(&item_path(item)?) {
                surface.push_clip_path(&clip, &Default::default());
                surface.push_transform(&krilla::geom::Transform::from_translate(dest.x, dest.y));
                let image = krilla::image::Image::from_rgba8(
                    decoded.as_raw().clone(),
                    decoded.width(),
                    decoded.height(),
                );
                surface.push_opacity(krilla::num::NormalizedF32::new(item.opacity).unwrap());
                surface.draw_image(
                    image,
                    krilla::geom::Size::from_wh(dest.width, dest.height)
                        .ok_or_else(|| invalid("invalid image dimensions"))?,
                );
                surface.pop();
                surface.pop();
                surface.pop();
            }
        }
        Draw::Table { cells, .. } => {
            for cell in cells {
                paint_pdf(cell, surface, resources, fonts)?;
            }
        }
        Draw::Chart { primitives, .. } => {
            for p in primitives {
                paint_pdf(p, surface, resources, fonts)?;
            }
        }
        _ => {}
    }
    Ok(())
}
