//! Native drawing of the resolved scene. Neither backend performs text layout.
use crate::model::*;
use crate::preset::PathFill;
use std::collections::{BTreeMap, btree_map::Entry};

/// An absolute path command in page units.
#[derive(Clone, Copy, Debug)]
pub enum Command {
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
        let (sx, sy) = (w / view_box[0], h / view_box[1]);
        let p = |a: f32, b: f32| (x + a * sx, y + b * sy);
        return crate::icon::path_commands(data)?
            .into_iter()
            .map(|c| {
                let c = match c {
                    Command::M(a, b) => {
                        let (a, b) = p(a, b);
                        Command::M(a, b)
                    }
                    Command::L(a, b) => {
                        let (a, b) = p(a, b);
                        Command::L(a, b)
                    }
                    Command::C(a, b, c, d, e, f) => {
                        let ((a, b), (c, d), (e, f)) = (p(a, b), p(c, d), p(e, f));
                        Command::C(a, b, c, d, e, f)
                    }
                    Command::Z => Command::Z,
                };
                match c {
                    Command::M(a, b) | Command::L(a, b) => {
                        finite(a, "path coordinate")?;
                        finite(b, "path coordinate")?;
                    }
                    Command::C(a, b, c, d, e, f) => {
                        for v in [a, b, c, d, e, f] {
                            finite(v, "path coordinate")?;
                        }
                    }
                    Command::Z => {}
                }
                Ok(c)
            })
            .collect();
    }
    if matches!(&item.kind, Draw::Shape(s) if s.is("line")) {
        return Ok(vec![Command::M(x, y), Command::L(x + w, y + h)]);
    }
    if matches!(&item.kind, Draw::Shape(s) if s.is("ellipse")) {
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
/// What an item paints: sub-paths with their fill shading and whether they are stroked.
/// Presets other than rect, ellipse and line come from the OOXML definitions.
fn outlines(item: &Item) -> Result<Vec<(Vec<Command>, PathFill, bool)>> {
    if let Draw::Shape(s) = &item.kind
        && !["rect", "ellipse", "line"].contains(&s.preset.as_str())
    {
        return Ok(crate::preset::geometry(&s.preset, item.rect, &s.adjust)?
            .into_iter()
            .map(|p| (p.commands, p.fill, p.stroke))
            .collect());
    }
    Ok(vec![(item_path(item)?, PathFill::Norm, true)])
}
fn rect_commands(r: Rect) -> [Command; 5] {
    [
        Command::M(r.x, r.y),
        Command::L(r.right(), r.y),
        Command::L(r.right(), r.bottom()),
        Command::L(r.x, r.bottom()),
        Command::Z,
    ]
}
/// Highlights go under the glyphs, decoration lines over them; link areas are not painted.
fn painted(marks: &[TextMark], under: bool) -> impl Iterator<Item = &TextMark> {
    marks.iter().filter(move |m| {
        if under {
            m.kind == MarkKind::Highlight
        } else {
            m.kind == MarkKind::Line
        }
    })
}
/// Mirroring then rotation about the item's center, in page units.
fn turn_transform(item: &Item) -> tiny_skia::Transform {
    let t = item.turn;
    if t.is_identity() {
        return tiny_skia::Transform::identity();
    }
    let (cx, cy) = (
        item.rect.x + item.rect.width / 2.0,
        item.rect.y + item.rect.height / 2.0,
    );
    tiny_skia::Transform::from_translate(cx, cy)
        .pre_concat(tiny_skia::Transform::from_rotate(t.rotate))
        .pre_scale(
            if t.flip_x { -1.0 } else { 1.0 },
            if t.flip_y { -1.0 } else { 1.0 },
        )
        .pre_translate(-cx, -cy)
}
/// The CSS gradient line: through the center, long enough that corners reach 0% and 100%.
fn gradient_line(r: Rect, angle: f32) -> ((f32, f32), (f32, f32)) {
    let (sin, cos) = angle.to_radians().sin_cos();
    let half = ((r.width * sin).abs() + (r.height * cos).abs()) / 2.0;
    let (cx, cy) = (r.x + r.width / 2.0, r.y + r.height / 2.0);
    (
        (cx - sin * half, cy + cos * half),
        (cx + sin * half, cy - cos * half),
    )
}
/// Radial gradients are circles from the center to the farthest corner.
fn gradient_radius(r: Rect) -> f32 {
    (r.width.powi(2) + r.height.powi(2)).sqrt() / 2.0
}
fn skia_gradient(item: &Item, g: &Gradient) -> Result<tiny_skia::Shader<'static>> {
    let stops = g
        .stops
        .iter()
        .map(|(at, c)| {
            tiny_skia::GradientStop::new(
                *at,
                tiny_skia::Color::from_rgba8(
                    c.0,
                    c.1,
                    c.2,
                    (c.3 as f32 * item.opacity).round() as u8,
                ),
            )
        })
        .collect::<Vec<_>>();
    let r = item.rect;
    let shader = if g.radial {
        let center = tiny_skia::Point::from_xy(r.x + r.width / 2.0, r.y + r.height / 2.0);
        tiny_skia::RadialGradient::new(
            center,
            0.0,
            center,
            gradient_radius(r),
            stops,
            tiny_skia::SpreadMode::Pad,
            tiny_skia::Transform::identity(),
        )
    } else {
        let (a, b) = gradient_line(r, g.angle);
        tiny_skia::LinearGradient::new(
            tiny_skia::Point::from_xy(a.0, a.1),
            tiny_skia::Point::from_xy(b.0, b.1),
            stops,
            tiny_skia::SpreadMode::Pad,
            tiny_skia::Transform::identity(),
        )
    };
    shader.ok_or_else(|| invalid("empty gradient area"))
}
/// The item's stroke for tiny-skia: width, dash, caps and joins.
fn skia_stroke(item: &Item) -> tiny_skia::Stroke {
    tiny_skia::Stroke {
        width: item.stroke_width,
        dash: dash_array(item).and_then(|d| tiny_skia::StrokeDash::new(d, 0.0)),
        line_cap: match item.line_cap {
            LineCap::Butt => tiny_skia::LineCap::Butt,
            LineCap::Round => tiny_skia::LineCap::Round,
            LineCap::Square => tiny_skia::LineCap::Square,
        },
        line_join: match item.line_join {
            LineJoin::Miter => tiny_skia::LineJoin::Miter,
            LineJoin::Round => tiny_skia::LineJoin::Round,
            LineJoin::Bevel => tiny_skia::LineJoin::Bevel,
        },
        ..Default::default()
    }
}
/// Dash lengths relative to the stroke width, as Office draws `dash` and `sysDot`.
fn dash_array(item: &Item) -> Option<Vec<f32>> {
    let w = item.stroke_width;
    match item.dash {
        Dash::Solid => None,
        Dash::Dashed => Some(vec![4.0 * w, 3.0 * w]),
        Dash::Dotted => Some(vec![w, w]),
    }
}
/// Closed outlines of the line-end markers, in page units before the item's turn.
fn marker_outlines(item: &Item) -> Result<Vec<Vec<Command>>> {
    if item.markers.iter().all(Option::is_none) {
        return Ok(Vec::new());
    }
    let points = outlines(item)?
        .into_iter()
        .find(|o| o.2)
        .map(|o| o.0)
        .unwrap_or_default()
        .into_iter()
        .flat_map(|c| match c {
            Command::M(x, y) | Command::L(x, y) => vec![(x, y)],
            Command::C(a, b, c, d, x, y) => vec![(a, b), (c, d), (x, y)],
            Command::Z => vec![],
        })
        .collect::<Vec<_>>();
    // An end's direction points away from the line, from the nearest distinct point.
    fn end<'p>(
        tip: (f32, f32),
        mut rest: impl Iterator<Item = &'p (f32, f32)>,
    ) -> Option<((f32, f32), (f32, f32))> {
        rest.find(|p| (p.0 - tip.0).hypot(p.1 - tip.1) > 1e-3)
            .map(|p| {
                let len = (tip.0 - p.0).hypot(tip.1 - p.1);
                (tip, ((tip.0 - p.0) / len, (tip.1 - p.1) / len))
            })
    }
    let ends = [
        points.first().and_then(|&t| end(t, points.iter().skip(1))),
        points
            .last()
            .and_then(|&t| end(t, points.iter().rev().skip(1))),
    ];
    let w = item.stroke_width.max(1.0);
    let (len, half) = (3.0 * w, 1.5 * w);
    let mut out = Vec::new();
    for (marker, end) in item.markers.iter().zip(ends) {
        let (Some(marker), Some(((tx, ty), (dx, dy)))) = (marker, end) else {
            continue;
        };
        let (nx, ny) = (-dy, dx);
        let at = |along: f32, across: f32| {
            (tx + dx * along + nx * across, ty + dy * along + ny * across)
        };
        let polygon = |points: &[(f32, f32)]| {
            let mut c = vec![Command::M(points[0].0, points[0].1)];
            c.extend(points[1..].iter().map(|p| Command::L(p.0, p.1)));
            c.push(Command::Z);
            c
        };
        out.push(match marker {
            Marker::Arrow => polygon(&[at(0.0, 0.0), at(-len, half), at(-len, -half)]),
            Marker::Stealth => polygon(&[
                at(0.0, 0.0),
                at(-len, half),
                at(-len * 0.5, 0.0),
                at(-len, -half),
            ]),
            Marker::Diamond => polygon(&[
                at(len / 2.0, 0.0),
                at(0.0, half),
                at(-len / 2.0, 0.0),
                at(0.0, -half),
            ]),
            Marker::Oval => item_path(&Item {
                rect: Rect {
                    x: tx - half,
                    y: ty - half,
                    width: 2.0 * half,
                    height: 2.0 * half,
                },
                kind: Draw::Shape(ShapeKind {
                    preset: "ellipse".into(),
                    adjust: Vec::new(),
                }),
                ..Item::default()
            })?,
        });
    }
    Ok(out)
}
/// The blurred shadow of an item at `scale`, with its device position.
fn shadow_layer(
    item: &Item,
    resources: &Resources,
    scale: f32,
) -> Result<Option<(tiny_skia::Pixmap, i32, i32)>> {
    let Some(s) = item.shadow else {
        return Ok(None);
    };
    if matches!(item.kind, Draw::Image { .. }) {
        // A picture casts the shadow of its clipped frame, as if filled.
        let frame = Item {
            fill: Color::BLACK,
            ..image_frame(item, resources)?
        };
        return shadow_layer(&frame, resources, scale);
    }
    let outline = outlines(item)?;
    let solid = item.fill.3 > 0 || item.gradient.is_some();
    // A filled item casts the shadow of its filled parts; an outline casts its strokes.
    let mut builder = tiny_skia::PathBuilder::new();
    for (commands, shade, stroked) in &outline {
        if (solid && *shade != PathFill::None || !solid && *stroked)
            && let Some(p) = skia_path(commands)
        {
            builder.push_path(&p);
        }
    }
    let Some(path) = builder
        .finish()
        .and_then(|p| p.transform(turn_transform(item)))
    else {
        return Ok(None);
    };
    let sigma = s.blur / 2.0 * scale;
    let pad = (sigma * 3.0).ceil() + 1.0;
    let b = path.bounds();
    let x0 = ((b.left() + s.dx) * scale - pad).floor();
    let y0 = ((b.top() + s.dy) * scale - pad).floor();
    let w = (b.width() * scale + 2.0 * pad).ceil() as u32 + 1;
    let h = (b.height() * scale + 2.0 * pad).ceil() as u32 + 1;
    if u64::from(w) * u64::from(h) > 64_000_000 {
        return Err(invalid("shadow exceeds pixel budget"));
    }
    let mut layer =
        tiny_skia::Pixmap::new(w, h).ok_or_else(|| invalid("cannot allocate shadow"))?;
    let transform = tiny_skia::Transform::from_translate(-x0, -y0)
        .pre_scale(scale, scale)
        .pre_translate(s.dx, s.dy);
    let color = paint(s.color, item.opacity);
    if solid {
        layer.fill_path(&path, &color, tiny_skia::FillRule::Winding, transform, None);
    } else {
        let stroke = tiny_skia::Stroke {
            width: item.stroke_width,
            ..Default::default()
        };
        layer.stroke_path(&path, &color, &stroke, transform, None);
    }
    blur(&mut layer, sigma);
    Ok(Some((layer, x0 as i32, y0 as i32)))
}
/// Approximate a Gaussian blur with three box blurs per axis.
fn blur(layer: &mut tiny_skia::Pixmap, sigma: f32) {
    if sigma < 0.5 {
        return;
    }
    let (w, h) = (layer.width() as usize, layer.height() as usize);
    // Box widths for three passes (W3C filter effects formula).
    let size = ((sigma * 3.0 * (2.0 * std::f32::consts::PI).sqrt() / 4.0) + 0.5).floor() as usize;
    let radius = (size / 2).max(1);
    let data = layer.data_mut();
    let mut line = Vec::new();
    for pass in 0..6 {
        let horizontal = pass % 2 == 0;
        let (outer, inner, stride, step) = if horizontal {
            (h, w, w * 4, 4)
        } else {
            (w, h, 4, w * 4)
        };
        for o in 0..outer {
            let base = o * stride;
            for channel in 0..4 {
                line.clear();
                line.extend((0..inner).map(|i| u32::from(data[base + i * step + channel])));
                let mut sum = 0u32;
                let window = (2 * radius + 1) as u32;
                for i in 0..inner + radius {
                    if i < inner {
                        sum += line[i];
                    }
                    if i > 2 * radius {
                        sum -= line[i - 2 * radius - 1];
                    }
                    if i >= radius {
                        data[base + (i - radius) * step + channel] = (sum / window) as u8;
                    }
                }
            }
        }
    }
    // Keep premultiplied invariants after integer rounding.
    for px in data.as_chunks_mut::<4>().0 {
        let a = px[3];
        for c in &mut px[..3] {
            *c = (*c).min(a);
        }
    }
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
    render_png_step(scene, page, resources, scale, &BTreeMap::new())
}

/// The page at one slide-show step: `looks` (from [`crate::show::steps`]) hide, fade,
/// recolor, scale or move animated elements and every element of an animated Group.
pub fn render_png_step(
    scene: &Scene,
    page: usize,
    resources: &Resources,
    scale: f32,
    looks: &BTreeMap<String, crate::show::Look>,
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
    let mut faces = Faces::new();
    for item in std::iter::once(&page.background).chain(&page.items) {
        // A Group's look applies to its members; scaling is about the animated object's center.
        let own = item.id.strip_suffix("::background").unwrap_or(&item.id);
        let owner = looks
            .get_key_value(own)
            .or_else(|| item.group.as_ref().and_then(|g| looks.get_key_value(g)));
        let Some((owner, look)) = owner else {
            paint_png(item, &mut pixmap, resources, &mut faces, scale)?;
            continue;
        };
        if look.hidden {
            continue;
        }
        let mut item = item.clone();
        item.opacity *= look.opacity;
        if let Some(c) = look.fill
            && !matches!(
                item.kind,
                Draw::Image { .. } | Draw::Table { .. } | Draw::Chart { .. } | Draw::Group
            )
        {
            item.fill = c;
            item.gradient = None;
        }
        if look.scale == 1.0 && look.offset == (0.0, 0.0) {
            paint_png(&item, &mut pixmap, resources, &mut faces, scale)?;
            continue;
        }
        // The animated object's full box: a Text's painted box when it has one.
        let painted = format!("{owner}::background");
        let r = page
            .items
            .iter()
            .find(|i| i.id == painted)
            .or_else(|| page.items.iter().find(|i| i.id == *owner))
            .map_or(item.rect, |i| i.rect);
        let (cx, cy) = (
            (r.x + r.width / 2.0) * scale,
            (r.y + r.height / 2.0) * scale,
        );
        let mut layer = tiny_skia::Pixmap::new(w, h)
            .ok_or_else(|| invalid("cannot allocate render surface"))?;
        paint_png(&item, &mut layer, resources, &mut faces, scale)?;
        let transform =
            tiny_skia::Transform::from_translate(look.offset.0 * scale, look.offset.1 * scale)
                .pre_translate(cx, cy)
                .pre_scale(look.scale, look.scale)
                .pre_translate(-cx, -cy);
        pixmap.draw_pixmap(
            0,
            0,
            layer.as_ref(),
            &tiny_skia::PixmapPaint {
                quality: tiny_skia::FilterQuality::Bilinear,
                ..Default::default()
            },
            transform,
            None,
        );
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
    if let Some((layer, x, y)) = shadow_layer(item, resources, scale)? {
        pixmap.draw_pixmap(
            x,
            y,
            layer.as_ref(),
            &tiny_skia::PixmapPaint::default(),
            tiny_skia::Transform::identity(),
            None,
        );
    }
    let transform = tiny_skia::Transform::from_scale(scale, scale).pre_concat(turn_transform(item));
    if (item.fill.3 > 0 || item.stroke_width > 0.0) && !matches!(item.kind, Draw::Image { .. }) {
        for (commands, shade, stroked) in outlines(item)? {
            let Some(path) = skia_path(&commands) else {
                continue;
            };
            if item.fill.3 > 0 && shade != PathFill::None {
                let mut fill = paint(shade.shade(item.fill), item.opacity);
                if let Some(g) = &item.gradient {
                    fill.shader = skia_gradient(item, g)?;
                }
                pixmap.fill_path(&path, &fill, tiny_skia::FillRule::Winding, transform, None);
            }
            if item.stroke_width > 0.0 && stroked {
                pixmap.stroke_path(
                    &path,
                    &paint(item.stroke, item.opacity),
                    &skia_stroke(item),
                    transform,
                    None,
                );
            }
        }
        if item.stroke_width > 0.0 {
            let stroke_paint = paint(item.stroke, item.opacity);
            for marker in marker_outlines(item)? {
                if let Some(path) = skia_path(&marker) {
                    pixmap.fill_path(
                        &path,
                        &stroke_paint,
                        tiny_skia::FillRule::Winding,
                        transform,
                        None,
                    );
                }
            }
        }
    }
    match &item.kind {
        Draw::Text { glyphs, marks, .. } => {
            let fill_marks = |pixmap: &mut tiny_skia::Pixmap, under: bool| {
                for m in painted(marks, under) {
                    if let Some(path) = skia_path(&rect_commands(m.rect)) {
                        pixmap.fill_path(
                            &path,
                            &paint(m.color, item.opacity),
                            tiny_skia::FillRule::Winding,
                            transform,
                            None,
                        );
                    }
                }
            };
            fill_marks(pixmap, true);
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
            fill_marks(pixmap, false);
        }
        Draw::Image { src, fit, .. } => {
            let bytes = resources
                .images
                .get(src)
                .ok_or_else(|| invalid(format!("missing image {src}")))?;
            let image = decode_image(bytes)?;
            let (w, h) = image.dimensions();
            let frame = image_frame(item, resources)?;
            let transform =
                tiny_skia::Transform::from_scale(scale, scale).pre_concat(turn_transform(&frame));
            let dest = image_rect(item.rect, w as f32, h as f32, *fit);
            let dw = (dest.width * scale).ceil().max(1.0) as u32;
            let dh = (dest.height * scale).ceil().max(1.0) as u32;
            if u64::from(dw) * u64::from(dh) > 64_000_000 {
                return Err(invalid("image render exceeds pixel budget"));
            }
            let resized =
                image::imageops::resize(&image, dw, dh, image::imageops::FilterType::Triangle);
            let mut layer =
                tiny_skia::Pixmap::new(dw, dh).ok_or_else(|| invalid("cannot allocate image"))?;
            for (px, pixel) in layer.pixels_mut().iter_mut().zip(resized.pixels()) {
                let alpha = (pixel[3] as f32 * item.opacity).round() as u8;
                *px = tiny_skia::ColorU8::from_rgba(pixel[0], pixel[1], pixel[2], alpha)
                    .premultiply();
            }
            // One device pixel per image pixel, anchored on the device grid.
            let pattern = tiny_skia::Pattern::new(
                layer.as_ref(),
                tiny_skia::SpreadMode::Pad,
                tiny_skia::FilterQuality::Nearest,
                1.0,
                tiny_skia::Transform::from_translate(
                    (dest.x * scale).round() / scale,
                    (dest.y * scale).round() / scale,
                )
                .pre_scale(1.0 / scale, 1.0 / scale),
            );
            let plain = frame.turn.is_identity()
                && matches!(&frame.kind, Draw::Shape(c) if c.is("rect"))
                && frame.radius == 0.0;
            if let Some(path) = skia_path(&fill_outline(&frame)?) {
                let mut paint = tiny_skia::Paint {
                    shader: pattern,
                    ..Default::default()
                };
                // Plain rectangles stay pixel-aligned; clipped or turned edges are smoothed.
                paint.anti_alias = !plain;
                pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, transform, None);
            }
            if item.stroke_width > 0.0 {
                for (commands, _, stroked) in outlines(&frame)? {
                    if let (true, Some(path)) = (stroked, skia_path(&commands)) {
                        pixmap.stroke_path(
                            &path,
                            &paint(item.stroke, item.opacity),
                            &skia_stroke(item),
                            transform,
                            None,
                        );
                    }
                }
            }
        }
        Draw::Icon(icon) => {
            let color = paint(item.text_style.color, item.opacity);
            let stroke = tiny_skia::Stroke {
                width: icon.stroke_width,
                line_cap: if icon.round {
                    tiny_skia::LineCap::Round
                } else {
                    tiny_skia::LineCap::Butt
                },
                line_join: if icon.round {
                    tiny_skia::LineJoin::Round
                } else {
                    tiny_skia::LineJoin::Miter
                },
                ..Default::default()
            };
            for p in &icon.paths {
                let Some(path) = skia_path(&p.commands) else {
                    continue;
                };
                if p.fill {
                    pixmap.fill_path(&path, &color, tiny_skia::FillRule::Winding, transform, None);
                }
                if p.stroke {
                    pixmap.stroke_path(&path, &color, &stroke, transform, None);
                }
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
/// An image's visible frame as a shape item: its clip outline, border, shadow and turn
/// all follow this frame, as a PPTX picture frame does.
fn image_frame(item: &Item, resources: &Resources) -> Result<Item> {
    let Draw::Image { src, fit, clip } = &item.kind else {
        return Ok(item.clone());
    };
    let bytes = resources
        .images
        .get(src)
        .ok_or_else(|| invalid(format!("missing image {src}")))?;
    let (w, h) = image_size(bytes)?;
    Ok(Item {
        rect: visible_image(item.rect, w as f32, h as f32, *fit),
        kind: Draw::Shape(clip.clone()),
        fill: Color(0, 0, 0, 0),
        gradient: None,
        ..item.clone()
    })
}
/// The filled area of a shape item, as one outline.
fn fill_outline(item: &Item) -> Result<Vec<Command>> {
    Ok(outlines(item)?
        .into_iter()
        .filter(|o| o.1 != PathFill::None)
        .flat_map(|o| o.0)
        .collect())
}
/// The part of the box an image actually covers under its object-fit.
fn visible_image(rect: Rect, w: f32, h: f32, fit: ImageFit) -> Rect {
    let dest = image_rect(rect, w, h, fit);
    let x = dest.x.max(rect.x);
    let y = dest.y.max(rect.y);
    Rect {
        x,
        y,
        width: dest.right().min(rect.right()) - x,
        height: dest.bottom().min(rect.bottom()) - y,
    }
}
/// Where the whole image is drawn: scaled per object-fit, placed per object-position.
pub(crate) fn image_rect(rect: Rect, w: f32, h: f32, fit: ImageFit) -> Rect {
    let ratio = match fit.mode {
        FitMode::Fill => return rect,
        FitMode::Cover => (rect.width / w).max(rect.height / h),
        FitMode::Contain => (rect.width / w).min(rect.height / h),
    };
    let (width, height) = (w * ratio, h * ratio);
    Rect {
        x: rect.x + (rect.width - width) * fit.position[0],
        y: rect.y + (rect.height - height) * fit.position[1],
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
        let mut links = Vec::new();
        for item in std::iter::once(&page.background).chain(&page.items) {
            paint_pdf(item, &mut surface, resources, &mut font_cache, &mut links)?;
        }
        surface.pop();
        surface.finish();
        for (r, href) in links {
            // Annotations use page coordinates: 1px = 0.75pt.
            if let Some(rect) = krilla::geom::Rect::from_xywh(
                r.x * 0.75,
                r.y * 0.75,
                r.width * 0.75,
                r.height * 0.75,
            ) {
                p.add_annotation(krilla::annotation::Annotation::new_link(
                    krilla::annotation::LinkAnnotation::new(
                        rect,
                        krilla::annotation::Target::Action(
                            krilla::action::LinkAction::new(href).into(),
                        ),
                    ),
                    None,
                ));
            }
        }
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
/// PDF has no blur, so shadows are embedded as images rasterized at this scale.
const PDF_SHADOW_SCALE: f32 = 2.0;
fn krilla_transform(t: tiny_skia::Transform) -> krilla::geom::Transform {
    krilla::geom::Transform::from_row(t.sx, t.ky, t.kx, t.sy, t.tx, t.ty)
}
fn paint_pdf(
    item: &Item,
    surface: &mut krilla::surface::Surface,
    resources: &Resources,
    fonts: &mut BTreeMap<(usize, u32), krilla::text::Font>,
    links: &mut Vec<(Rect, String)>,
) -> Result<()> {
    if let Some((layer, x, y)) = shadow_layer(item, resources, PDF_SHADOW_SCALE)? {
        let mut rgba = Vec::with_capacity(layer.data().len());
        for px in layer.pixels() {
            let c = px.demultiply();
            rgba.extend([c.red(), c.green(), c.blue(), c.alpha()]);
        }
        let image = krilla::image::Image::from_rgba8(rgba, layer.width(), layer.height());
        surface.push_transform(&krilla::geom::Transform::from_translate(
            x as f32 / PDF_SHADOW_SCALE,
            y as f32 / PDF_SHADOW_SCALE,
        ));
        surface.draw_image(
            image,
            krilla::geom::Size::from_wh(
                layer.width() as f32 / PDF_SHADOW_SCALE,
                layer.height() as f32 / PDF_SHADOW_SCALE,
            )
            .ok_or_else(|| invalid("invalid shadow size"))?,
        );
        surface.pop();
    }
    let turned = !item.turn.is_identity();
    if turned {
        // A picture turns about its frame, like the PPTX picture frame.
        let basis = image_frame(item, resources)?;
        surface.push_transform(&krilla_transform(turn_transform(&basis)));
    }
    let painted = paint_pdf_item(item, surface, resources, fonts, links);
    if turned {
        surface.pop();
    }
    painted
}
fn paint_pdf_item(
    item: &Item,
    surface: &mut krilla::surface::Surface,
    resources: &Resources,
    fonts: &mut BTreeMap<(usize, u32), krilla::text::Font>,
    links: &mut Vec<(Rect, String)>,
) -> Result<()> {
    let gradient = if let Some(g) = &item.gradient {
        let r = item.rect;
        let stops = g
            .stops
            .iter()
            .map(|(at, c)| krilla::paint::Stop {
                offset: krilla::num::NormalizedF32::new(*at).unwrap(),
                color: krilla::color::rgb::Color::new(c.0, c.1, c.2).into(),
                opacity: krilla::num::NormalizedF32::new(c.3 as f32 / 255.0 * item.opacity)
                    .unwrap(),
            })
            .collect();
        let paint: krilla::paint::Paint = if g.radial {
            let (cx, cy) = (r.x + r.width / 2.0, r.y + r.height / 2.0);
            krilla::paint::RadialGradient {
                fx: cx,
                fy: cy,
                fr: 0.0,
                cx,
                cy,
                cr: gradient_radius(r),
                transform: Default::default(),
                spread_method: Default::default(),
                stops,
                anti_alias: true,
            }
            .into()
        } else {
            let (a, b) = gradient_line(r, g.angle);
            krilla::paint::LinearGradient {
                x1: a.0,
                y1: a.1,
                x2: b.0,
                y2: b.1,
                transform: Default::default(),
                spread_method: Default::default(),
                anti_alias: true,
                stops,
            }
            .into()
        };
        Some(krilla::paint::Fill {
            paint,
            opacity: krilla::num::NormalizedF32::ONE,
            rule: Default::default(),
        })
    } else {
        None
    };
    let stroke = if item.stroke_width > 0.0 {
        Some(krilla::paint::Stroke {
            paint: krilla::color::rgb::Color::new(item.stroke.0, item.stroke.1, item.stroke.2)
                .into(),
            width: item.stroke_width,
            opacity: krilla::num::NormalizedF32::new(item.stroke.3 as f32 / 255.0 * item.opacity)
                .unwrap(),
            dash: dash_array(item).map(|array| krilla::paint::StrokeDash { array, offset: 0.0 }),
            line_cap: match item.line_cap {
                LineCap::Butt => krilla::paint::LineCap::Butt,
                LineCap::Round => krilla::paint::LineCap::Round,
                LineCap::Square => krilla::paint::LineCap::Square,
            },
            line_join: match item.line_join {
                LineJoin::Miter => krilla::paint::LineJoin::Miter,
                LineJoin::Round => krilla::paint::LineJoin::Round,
                LineJoin::Bevel => krilla::paint::LineJoin::Bevel,
            },
            ..Default::default()
        })
    } else {
        None
    };
    if (item.fill.3 > 0 || item.stroke_width > 0.0) && !matches!(item.kind, Draw::Image { .. }) {
        for (commands, shade, stroked) in outlines(item)? {
            let Some(path) = pdf_path(&commands) else {
                continue;
            };
            surface.set_fill(if item.fill.3 == 0 || shade == PathFill::None {
                None
            } else if gradient.is_some() {
                gradient.clone()
            } else {
                Some(pdf_fill(shade.shade(item.fill), item.opacity))
            });
            surface.set_stroke(if stroked { stroke.clone() } else { None });
            surface.draw_path(&path);
        }
    }
    let markers = marker_outlines(item)?;
    if !markers.is_empty() {
        surface.set_stroke(None);
        surface.set_fill(Some(pdf_fill(item.stroke, item.opacity)));
        for marker in markers {
            if let Some(path) = pdf_path(&marker) {
                surface.draw_path(&path);
            }
        }
    }
    match &item.kind {
        Draw::Text { glyphs, marks, .. } => {
            surface.set_stroke(None);
            let fill_marks = |surface: &mut krilla::surface::Surface, under: bool| {
                for m in painted(marks, under) {
                    if let Some(path) = pdf_path(&rect_commands(m.rect)) {
                        surface.set_fill(Some(pdf_fill(m.color, item.opacity)));
                        surface.draw_path(&path);
                    }
                }
            };
            fill_marks(surface, true);
            for m in marks {
                if let MarkKind::Link(href) = &m.kind {
                    links.push((
                        crate::layout::turned_bounds(m.rect, item.turn),
                        href.clone(),
                    ));
                }
            }
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
            fill_marks(surface, false);
        }
        Draw::Image { src, fit, .. } => {
            let data = resources
                .images
                .get(src)
                .ok_or_else(|| invalid("missing image"))?;
            let decoded = decode_image(data)?;
            let dest = image_rect(
                item.rect,
                decoded.width() as f32,
                decoded.height() as f32,
                *fit,
            );
            let frame = image_frame(item, resources)?;
            if let Some(clip) = pdf_path(&fill_outline(&frame)?) {
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
            if stroke.is_some() {
                surface.set_fill(None);
                surface.set_stroke(stroke.clone());
                for (commands, _, stroked) in outlines(&frame)? {
                    if let (true, Some(path)) = (stroked, pdf_path(&commands)) {
                        surface.draw_path(&path);
                    }
                }
            }
        }
        Draw::Icon(icon) => {
            let c = item.text_style.color;
            let stroke = krilla::paint::Stroke {
                paint: krilla::color::rgb::Color::new(c.0, c.1, c.2).into(),
                width: icon.stroke_width,
                opacity: krilla::num::NormalizedF32::new(c.3 as f32 / 255.0 * item.opacity)
                    .unwrap(),
                line_cap: if icon.round {
                    krilla::paint::LineCap::Round
                } else {
                    krilla::paint::LineCap::Butt
                },
                line_join: if icon.round {
                    krilla::paint::LineJoin::Round
                } else {
                    krilla::paint::LineJoin::Miter
                },
                ..Default::default()
            };
            for p in &icon.paths {
                if let Some(path) = pdf_path(&p.commands) {
                    surface.set_fill(p.fill.then(|| pdf_fill(c, item.opacity)));
                    surface.set_stroke(p.stroke.then(|| stroke.clone()));
                    surface.draw_path(&path);
                }
            }
        }
        Draw::Table { cells, .. } => {
            for cell in cells {
                paint_pdf(cell, surface, resources, fonts, links)?;
            }
        }
        Draw::Chart { primitives, .. } => {
            for p in primitives {
                paint_pdf(p, surface, resources, fonts, links)?;
            }
        }
        _ => {}
    }
    Ok(())
}
