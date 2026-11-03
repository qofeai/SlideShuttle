use crate::model::Rect;
use crate::model::*;
use crate::text::{TextEngine, text_style};
use std::collections::BTreeMap;
use taffy::prelude::*;

#[derive(Clone)]
struct Leaf {
    kind: Kind,
    text: TextStyle,
    wrap: bool,
}

pub fn layout(deck: &Deck, resources: &Resources) -> Result<Scene> {
    let mut text = TextEngine::new(resources)?;
    layout_with_text(deck, resources, &mut text)
}

pub(crate) fn layout_with_text(
    deck: &Deck,
    resources: &Resources,
    text: &mut TextEngine,
) -> Result<Scene> {
    let mut scene = Scene {
        width: deck.width,
        height: deck.height,
        title: deck.title.clone(),
        pages: Vec::new(),
        diagnostics: Vec::new(),
    };
    for slide in &deck.slides {
        let mut tree: TaffyTree<Leaf> = TaffyTree::new();
        tree.disable_rounding();
        let mut mapping = BTreeMap::new();
        let mut computed = BTreeMap::new();
        let base = TextStyle {
            family: deck.font_family.clone(),
            ..TextStyle::default()
        };
        let roots = slide
            .nodes
            .iter()
            .map(|n| {
                build(n, &base, &mut tree, &mut mapping, &mut computed, false)
                    .map_err(|e| at(&n.source, e))
            })
            .collect::<Result<Vec<_>>>()?;
        let root = tree
            .new_with_children(
                taffy::Style {
                    display: Display::Block,
                    size: Size {
                        width: Dimension::length(deck.width),
                        height: Dimension::length(deck.height),
                    },
                    ..Default::default()
                },
                &roots,
            )
            .map_err(|e| invalid(e.to_string()))?;
        tree.compute_layout_with_measure(
            root,
            Size {
                width: AvailableSpace::Definite(deck.width),
                height: AvailableSpace::Definite(deck.height),
            },
            |inputs, _, context, style| {
                taffy::compute::compute_leaf_layout(
                    inputs,
                    style,
                    |_, _| 0.0,
                    |known, available| {
                        let Some(leaf) = context else {
                            return Size::ZERO;
                        };
                        let width = known.width.or(match available.width {
                            AvailableSpace::Definite(v) => Some(v),
                            AvailableSpace::MinContent => Some(1.0),
                            _ => None,
                        });
                        let (w, h) = match &leaf.kind {
                            Kind::Text { paragraphs } => {
                                match text.shape(paragraphs, &leaf.text, width, leaf.wrap) {
                                    Ok(s) => (s.width, s.height),
                                    Err(_) => (0.0, leaf.text.line_height),
                                }
                            }
                            Kind::Image { src, .. } => resources
                                .images
                                .get(src)
                                .and_then(|b| crate::render::image_size(b).ok())
                                .map(|(w, h)| (w as f32, h as f32))
                                .unwrap_or((100.0, 100.0)),
                            Kind::Table { rows, .. } => (
                                width.unwrap_or(600.0),
                                rows.len() as f32 * (leaf.text.line_height + 20.0),
                            ),
                            Kind::Chart { .. } => (width.unwrap_or(600.0), 320.0),
                            // Outline icon sets are drawn on a 24px grid.
                            Kind::Icon { .. } => (24.0, 24.0),
                            _ => (0.0, 0.0),
                        };
                        Size {
                            width: known.width.unwrap_or(w),
                            height: known.height.unwrap_or(h),
                        }
                    },
                )
            },
        )
        .map_err(|e| invalid(e.to_string()))?;
        let mut emitter = Emitter {
            page: &slide.id,
            tree: &tree,
            mapping: &mapping,
            computed: &computed,
            text: &mut *text,
            resources,
            items: Vec::new(),
            diagnostics: &mut scene.diagnostics,
        };
        for node in &slide.nodes {
            emitter
                .emit(node, 0.0, 0.0, None)
                .map_err(|e| at(&node.source, e))?;
        }
        let page = ScenePage {
            id: slide.id.clone(),
            background: page_background(&slide.background, deck, resources)?,
            items: emitter.items,
        };
        for item in &page.items {
            let r = turned_bounds(item.rect, item.turn);
            if r.x < -0.5
                || r.y < -0.5
                || r.right() > deck.width + 0.5
                || r.bottom() > deck.height + 0.5
            {
                scene.diagnostics.push(diag(
                    "out_of_bounds",
                    Severity::Error,
                    &slide.id,
                    item,
                    "element extends outside the slide",
                ));
            }
        }
        scene.pages.push(page);
    }
    Ok(scene)
}

/// `object-fit` and `object-position` (keywords or percentages per axis, default centered).
fn image_fit(style: &Css) -> Result<ImageFit> {
    let mode = style
        .get("object-fit")
        .map(|v| keyword(v, "object-fit"))
        .transpose()?
        .unwrap_or_default();
    let mut position = [0.5, 0.5];
    if let Some(v) = style.get("object-position") {
        let words = v.split_whitespace().collect::<Vec<_>>();
        if words.is_empty() || words.len() > 2 {
            return Err(invalid("object-position takes one or two values"));
        }
        for (i, word) in words.iter().enumerate() {
            let (axis, at) = match *word {
                "left" => (0, 0.0),
                "right" => (0, 1.0),
                "top" => (1, 0.0),
                "bottom" => (1, 1.0),
                "center" => (i, 0.5),
                p => (
                    i,
                    p.strip_suffix('%')
                        .and_then(|n| n.parse::<f32>().ok())
                        .filter(|n| (0.0..=100.0).contains(n))
                        .ok_or_else(|| {
                            invalid(format!(
                                "object-position value {p} must be a keyword or 0%–100%"
                            ))
                        })?
                        / 100.0,
                ),
            };
            position[axis] = at;
        }
    }
    Ok(ImageFit { mode, position })
}
/// The page background: a color, gradient, or `url(<image>)` covering the page.
fn page_background(value: &str, deck: &Deck, resources: &Resources) -> Result<Item> {
    let mut item = Item {
        id: "background".into(),
        rect: Rect {
            x: 0.0,
            y: 0.0,
            width: deck.width,
            height: deck.height,
        },
        ..Item::default()
    };
    if let Some(src) = background_image(value) {
        if !resources.images.contains_key(src) {
            return Err(invalid(format!("missing background image {src}")));
        }
        item.kind = Draw::Image {
            src: src.into(),
            fit: ImageFit {
                mode: FitMode::Cover,
                ..ImageFit::default()
            },
            clip: ShapeKind::default(),
        };
    } else {
        (item.fill, item.gradient) = paint(value)?;
    }
    Ok(item)
}
/// The image of a `url(<image>)` background.
pub fn background_image(value: &str) -> Option<&str> {
    value
        .trim()
        .strip_prefix("url(")
        .and_then(|v| v.strip_suffix(')'))
        .map(|v| v.trim().trim_matches(['"', '\'']))
}
fn build(
    n: &Node,
    parent: &TextStyle,
    tree: &mut TaffyTree<Leaf>,
    mapping: &mut BTreeMap<String, NodeId>,
    computed: &mut BTreeMap<String, TextStyle>,
    in_group: bool,
) -> Result<NodeId> {
    let text = text_style(&n.style, parent)?;
    computed.insert(n.id.clone(), text.clone());
    if matches!(n.kind, Kind::Group) && in_group {
        return Err(invalid("nested native Group is not supported"));
    }
    let children = n
        .children
        .iter()
        .map(|c| {
            build(
                c,
                &text,
                tree,
                mapping,
                computed,
                in_group || matches!(n.kind, Kind::Group),
            )
            .map_err(|e| at(&c.source, e))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut style = box_style(&n.style)?;
    // A line's or path's border is its stroke, drawn along the geometry; it takes no box space.
    if match &n.kind {
        Kind::Shape { shape } => shape.is_open(),
        Kind::Path { .. } => true,
        _ => false,
    } {
        style.border = taffy::geometry::Rect::zero();
    }
    let context = Leaf {
        kind: n.kind.clone(),
        text,
        wrap: n.style.get("white-space").is_none_or(|s| s != "nowrap"),
    };
    let id = if children.is_empty() {
        tree.new_leaf_with_context(style, context)
    } else {
        tree.new_with_children(style, &children)
    }
    .map_err(|e| invalid(e.to_string()))?;
    mapping.insert(n.id.clone(), id);
    Ok(id)
}

fn dimension(s: &str) -> Result<Dimension> {
    if s == "auto" {
        return Ok(Dimension::auto());
    }
    if let Some(n) = s.strip_suffix('%') {
        let v = finite(n.parse().map_err(|_| invalid("invalid percentage"))?, s)?;
        return Ok(Dimension::percent(v / 100.0));
    }
    Ok(Dimension::length(length_px(s)?))
}
fn lp(s: &str) -> Result<LengthPercentage> {
    if let Some(n) = s.strip_suffix('%') {
        Ok(LengthPercentage::percent(
            finite(n.parse().map_err(|_| invalid("invalid percentage"))?, s)? / 100.0,
        ))
    } else {
        Ok(LengthPercentage::length(length_px(s)?))
    }
}
fn lpa(s: &str) -> Result<LengthPercentageAuto> {
    if s == "auto" {
        Ok(LengthPercentageAuto::auto())
    } else if let Some(n) = s.strip_suffix('%') {
        Ok(LengthPercentageAuto::percent(
            finite(n.parse().map_err(|_| invalid("invalid percentage"))?, s)? / 100.0,
        ))
    } else {
        Ok(LengthPercentageAuto::length(length_px(s)?))
    }
}
fn edges<T: Clone>(s: &str, parse: impl Fn(&str) -> Result<T>) -> Result<taffy::geometry::Rect<T>> {
    let a = s
        .split_whitespace()
        .map(parse)
        .collect::<Result<Vec<_>>>()?;
    let (t, r, b, l) = match a.as_slice() {
        [v] => (v, v, v, v),
        [t, r] => (t, r, t, r),
        [t, r, b] => (t, r, b, r),
        [t, r, b, l] => (t, r, b, l),
        _ => return Err(invalid("expected 1–4 box lengths")),
    };
    Ok(taffy::geometry::Rect {
        top: t.clone(),
        right: r.clone(),
        bottom: b.clone(),
        left: l.clone(),
    })
}
fn align(s: &str) -> Result<AlignItems> {
    Ok(match s {
        "start" | "flex-start" => AlignItems::START,
        "end" | "flex-end" => AlignItems::END,
        "center" => AlignItems::CENTER,
        "stretch" => AlignItems::STRETCH,
        "baseline" => AlignItems::BASELINE,
        _ => return Err(invalid(format!("unsupported alignment {s}"))),
    })
}
fn tracks(s: &str) -> Result<Vec<TrackSizingFunction>> {
    s.split_whitespace()
        .map(|v| {
            if v == "auto" {
                Ok(auto())
            } else if let Some(n) = v.strip_suffix("fr") {
                let n = finite(n.parse().map_err(|_| invalid("invalid fr track"))?, v)?;
                if n <= 0.0 {
                    return Err(invalid("fr must be positive"));
                }
                Ok(fr(n))
            } else {
                Ok(length(length_px(v)?))
            }
        })
        .collect()
}
fn placement(s: &str) -> Result<Line<GridPlacement>> {
    let parts = s.split('/').map(str::trim).collect::<Vec<_>>();
    let one = |v: &str| -> Result<GridPlacement> {
        if v == "auto" {
            Ok(GridPlacement::Auto)
        } else if let Some(n) = v.strip_prefix("span ") {
            let n = n.parse::<u16>().map_err(|_| invalid("invalid grid span"))?;
            if n == 0 {
                return Err(invalid("grid span must be positive"));
            }
            Ok(GridPlacement::Span(n))
        } else {
            let n = v.parse::<i16>().map_err(|_| invalid("invalid grid line"))?;
            if n == 0 {
                return Err(invalid("grid lines cannot be zero"));
            }
            Ok(GridPlacement::Line(n.into()))
        }
    };
    if parts.len() > 2 {
        return Err(invalid("invalid grid placement"));
    }
    Ok(Line {
        start: one(parts[0])?,
        end: if parts.len() == 2 {
            one(parts[1])?
        } else {
            GridPlacement::Auto
        },
    })
}
fn box_style(css: &Css) -> Result<taffy::Style> {
    let mut s = taffy::Style {
        display: Display::Block,
        ..Default::default()
    };
    for (k, v) in css {
        match k.as_str() {
            "display" => {
                s.display = match v.as_str() {
                    "block" => Display::Block,
                    "flex" => Display::Flex,
                    "grid" => Display::Grid,
                    _ => return Err(invalid("display supports block, flex, grid")),
                }
            }
            "width" => s.size.width = dimension(v)?,
            "height" => s.size.height = dimension(v)?,
            "min-width" => s.min_size.width = lpa(v)?,
            "max-width" => s.max_size.width = lpa(v)?,
            "min-height" => s.min_size.height = lpa(v)?,
            "max-height" => s.max_size.height = lpa(v)?,
            "padding-top" => s.padding.top = lp(v)?,
            "padding-right" => s.padding.right = lp(v)?,
            "padding-bottom" => s.padding.bottom = lp(v)?,
            "padding-left" => s.padding.left = lp(v)?,
            "margin-top" => s.margin.top = lpa(v)?,
            "margin-right" => s.margin.right = lpa(v)?,
            "margin-bottom" => s.margin.bottom = lpa(v)?,
            "margin-left" => s.margin.left = lpa(v)?,
            "row-gap" => s.gap.height = lp(v)?,
            "column-gap" => s.gap.width = lp(v)?,
            "position" => {
                s.position = match v.as_str() {
                    "relative" => Position::Relative,
                    "absolute" => Position::Absolute,
                    _ => return Err(invalid("position supports relative, absolute")),
                }
            }
            "left" => s.inset.left = lpa(v)?,
            "right" => s.inset.right = lpa(v)?,
            "top" => s.inset.top = lpa(v)?,
            "bottom" => s.inset.bottom = lpa(v)?,
            "flex-direction" => {
                s.flex_direction = match v.as_str() {
                    "row" => FlexDirection::Row,
                    "column" => FlexDirection::Column,
                    _ => return Err(invalid("flex-direction supports row or column")),
                }
            }
            "flex-grow" => s.flex_grow = nonnegative(v)?,
            "flex-shrink" => s.flex_shrink = nonnegative(v)?,
            "flex-basis" => s.flex_basis = dimension(v)?,
            "align-items" => s.align_items = align(v)?,
            "align-self" => s.align_self = Some(align(v)?),
            "justify-content" => {
                s.justify_content = match v.as_str() {
                    "start" | "flex-start" => JustifyContent::START,
                    "end" | "flex-end" => JustifyContent::END,
                    "center" => JustifyContent::CENTER,
                    "space-between" => JustifyContent::SPACE_BETWEEN,
                    "space-around" => JustifyContent::SPACE_AROUND,
                    "space-evenly" => JustifyContent::SPACE_EVENLY,
                    _ => return Err(invalid("unsupported justify-content")),
                }
            }
            "grid-template-columns" => {
                s.grid_template_columns = tracks(v)?.into_iter().map(Into::into).collect()
            }
            "grid-template-rows" => {
                s.grid_template_rows = tracks(v)?.into_iter().map(Into::into).collect()
            }
            "grid-column" => s.grid_column = placement(v)?,
            "grid-row" => s.grid_row = placement(v)?,
            "border-width" => s.border = edges(v, lp)?,
            "box-sizing" => {
                if v != "border-box" {
                    return Err(invalid("only border-box is supported"));
                }
            }
            "white-space" => {
                if !["normal", "nowrap", "pre-wrap"].contains(&v.as_str()) {
                    return Err(invalid("unsupported white-space"));
                }
            }
            "object-fit" if !["contain", "cover", "fill"].contains(&v.as_str()) => {
                return Err(invalid("unsupported object-fit"));
            }
            _ => {}
        }
    }
    Ok(s)
}
fn nonnegative(s: &str) -> Result<f32> {
    let n = finite(
        s.parse::<f32>()
            .map_err(|_| invalid(format!("invalid number {s}")))?,
        s,
    )?;
    if n < 0.0 {
        Err(invalid("negative value is not allowed"))
    } else {
        Ok(n)
    }
}
/// Paint, outline, shadow and transform of a node.
fn visual(n: &Node, item: &mut Item) -> Result<()> {
    let style = |k: &str| n.style.get(k).map(String::as_str);
    (item.fill, item.gradient) = paint(style("background").unwrap_or("transparent"))?;
    item.stroke = style("border-color")
        .map(Color::parse)
        .transpose()?
        .unwrap_or(Color::BLACK);
    item.stroke_width = style("border-width")
        .map(length_px)
        .transpose()?
        .unwrap_or(0.0);
    item.dash = match style("border-style") {
        None | Some("solid") => Dash::Solid,
        Some("dashed") => Dash::Dashed,
        Some("dotted") => Dash::Dotted,
        Some(other) => {
            return Err(invalid(format!(
                "border-style supports solid, dashed and dotted, not {other}"
            )));
        }
    };
    item.line_cap = style("stroke-linecap")
        .map(|v| keyword(v, "stroke-linecap"))
        .transpose()?
        .unwrap_or_default();
    item.line_join = style("stroke-linejoin")
        .map(|v| keyword(v, "stroke-linejoin"))
        .transpose()?
        .unwrap_or_default();
    item.radius = match style("border-radius") {
        // A percentage is of the shorter side, so 50% makes a circle or a pill.
        Some(v) if v.ends_with('%') => {
            let pct = v[..v.len() - 1]
                .trim()
                .parse::<f32>()
                .ok()
                .filter(|p| (0.0..=50.0).contains(p))
                .ok_or_else(|| invalid("border-radius percentage must be 0%–50%"))?;
            item.rect.width.min(item.rect.height) * pct / 100.0
        }
        Some(v) => length_px(v)?,
        None => 0.0,
    };
    item.opacity = style("opacity")
        .map(nonnegative)
        .transpose()?
        .unwrap_or(1.0);
    item.shadow = style("box-shadow").map(shadow).transpose()?.flatten();
    item.turn = style("transform")
        .map(turn)
        .transpose()?
        .unwrap_or_default();
    for (slot, key) in item.markers.iter_mut().zip(["marker-start", "marker-end"]) {
        *slot = style(key)
            .filter(|v| *v != "none")
            .map(|v| keyword(v, "line marker"))
            .transpose()?;
    }
    if item.opacity > 1.0 || item.radius < 0.0 || item.stroke_width < 0.0 {
        return Err(invalid("invalid opacity, radius or border width"));
    }
    let leaf = n.children.is_empty();
    if n.style.contains_key("vertical-align") && !matches!(n.kind, Kind::Text { .. }) {
        return Err(invalid(
            "vertical-align (top, middle, bottom) applies to Text",
        ));
    }
    if !leaf && item.opacity != 1.0 {
        return Err(invalid(
            "container opacity is not supported; apply opacity to leaves",
        ));
    }
    if !leaf && !item.turn.is_identity() {
        return Err(invalid(
            "transform applies to leaves; containers cannot be rotated",
        ));
    }
    let boxed = item.fill.3 > 0 || item.stroke_width > 0.0;
    if matches!(n.kind, Kind::Text { .. }) && boxed && !item.turn.is_identity() {
        return Err(invalid(
            "a rotated Text cannot have a background or border; rotate a Shape behind it",
        ));
    }
    if matches!(n.kind, Kind::Text { .. }) && !boxed && item.shadow.is_some() {
        return Err(invalid(
            "box-shadow needs the Text's box to be visible: give it a background or border",
        ));
    }
    let open_line = match &n.kind {
        Kind::Shape { shape } => shape.is_open(),
        Kind::Path { .. } => true,
        _ => false,
    };
    if item.markers.iter().any(Option::is_some) && !open_line {
        return Err(invalid(
            "marker-start/marker-end apply to lines, connectors and Paths",
        ));
    }
    if matches!(n.kind, Kind::Icon { .. })
        && (item.fill.3 > 0
            || item.gradient.is_some()
            || item.stroke_width > 0.0
            || item.shadow.is_some())
    {
        return Err(invalid(
            "an Icon is colored with color; put backgrounds, borders and shadows on a Box behind it",
        ));
    }
    if let Kind::Image { clip, .. } = &n.kind {
        if item.fill.3 > 0 || item.gradient.is_some() {
            return Err(invalid(
                "an Image has no background; put one on a Box behind it",
            ));
        }
        if item.radius > 0.0 && !clip.is("rect") {
            return Err(invalid(
                "border-radius rounds a rect image; other clip shapes use adjust",
            ));
        }
    }
    if matches!(n.kind, Kind::Table { .. } | Kind::Chart { .. })
        && (item.gradient.is_some()
            || item.opacity != 1.0
            || item.shadow.is_some()
            || !item.turn.is_identity())
    {
        return Err(invalid(
            "table/chart gradient, opacity, shadow or transform is not supported",
        ));
    }
    Ok(())
}
/// A color, `linear-gradient(...)` or `radial-gradient(...)`.
pub(crate) fn paint(value: &str) -> Result<(Color, Option<Gradient>)> {
    let value = value.trim();
    for (prefix, radial) in [("linear-gradient(", false), ("radial-gradient(", true)] {
        if let Some(args) = value.strip_prefix(prefix).and_then(|v| v.strip_suffix(')')) {
            let g = gradient(args, radial)?;
            return Ok((g.stops[0].1, Some(g)));
        }
    }
    Ok((Color::parse(value)?, None))
}
fn gradient(args: &str, radial: bool) -> Result<Gradient> {
    let mut parts = args.split(',').map(str::trim).collect::<Vec<_>>();
    let mut angle = 180.0;
    let first = parts.first().copied().unwrap_or("");
    if radial {
        if first == "circle" {
            parts.remove(0);
        }
    } else if let Some(deg) = first.strip_suffix("deg") {
        angle = finite(
            deg.trim()
                .parse::<f32>()
                .map_err(|_| invalid("invalid gradient angle"))?,
            "gradient angle",
        )?
        .rem_euclid(360.0);
        parts.remove(0);
    } else if let Some(side) = first.strip_prefix("to ") {
        angle = match side {
            "top" => 0.0,
            "right" => 90.0,
            "bottom" => 180.0,
            "left" => 270.0,
            _ => {
                return Err(invalid(
                    "gradient direction is to top/right/bottom/left or an angle in deg",
                ));
            }
        };
        parts.remove(0);
    }
    if !(2..=8).contains(&parts.len()) {
        return Err(invalid("a gradient needs 2–8 color stops"));
    }
    let mut stops = Vec::new();
    let mut given = Vec::new();
    for part in parts {
        let mut words = part.split_whitespace();
        let color = Color::parse(words.next().unwrap_or(""))?;
        let at = words
            .next()
            .map(|p| {
                p.strip_suffix('%')
                    .and_then(|n| n.parse::<f32>().ok())
                    .filter(|n| (0.0..=100.0).contains(n))
                    .map(|n| n / 100.0)
                    .ok_or_else(|| invalid(format!("gradient stop position must be 0%–100%: {p}")))
            })
            .transpose()?;
        if words.next().is_some() {
            return Err(invalid(
                "a gradient stop is a color and an optional percentage",
            ));
        }
        stops.push((0.0, color));
        given.push(at);
    }
    // Missing positions: first 0%, last 100%, others evenly between their neighbours.
    let last = given.len() - 1;
    given[0] = given[0].or(Some(0.0));
    given[last] = given[last].or(Some(1.0));
    let mut i = 0;
    while i < last {
        let next = (i + 1..=last).find(|&j| given[j].is_some()).unwrap();
        let (a, b) = (given[i].unwrap(), given[next].unwrap());
        let span = (next - i) as f32;
        for (k, slot) in given[i + 1..next].iter_mut().enumerate() {
            *slot = Some(a + (b - a) * (k + 1) as f32 / span);
        }
        i = next;
    }
    for (stop, at) in stops.iter_mut().zip(given) {
        stop.0 = at.unwrap();
    }
    if stops.windows(2).any(|w| w[1].0 < w[0].0) {
        return Err(invalid("gradient stop positions must not decrease"));
    }
    Ok(Gradient {
        radial,
        angle,
        stops,
    })
}
/// `box-shadow: <x> <y> [<blur>] <color>` or `none`.
fn shadow(value: &str) -> Result<Option<Shadow>> {
    if value.trim() == "none" {
        return Ok(None);
    }
    let words = value.split_whitespace().collect::<Vec<_>>();
    let (lengths, color) = match words.as_slice() {
        [lengths @ .., color] if (2..=3).contains(&lengths.len()) => (lengths, color),
        _ => return Err(invalid("box-shadow syntax: <x> <y> [<blur>] <color>")),
    };
    let lengths = lengths
        .iter()
        .map(|v| length_px(v))
        .collect::<Result<Vec<_>>>()?;
    let blur = lengths.get(2).copied().unwrap_or(0.0);
    if !(0.0..=200.0).contains(&blur) || lengths[..2].iter().any(|v| v.abs() > 1000.0) {
        return Err(invalid(
            "box-shadow blur must be 0–200px and offsets within ±1000px",
        ));
    }
    Ok(Some(Shadow {
        dx: lengths[0],
        dy: lengths[1],
        blur,
        color: Color::parse(color)?,
    }))
}
/// `transform`: `rotate(<deg>)`, `scaleX(-1)` and `scaleY(-1)`, applied right to left as
/// in CSS, normalized to mirroring first, then rotation (the OOXML order).
fn turn(value: &str) -> Result<Turn> {
    let mut t = Turn::default();
    if value.trim() == "none" {
        return Ok(t);
    }
    let functions = value
        .split(')')
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .collect::<Vec<_>>();
    for f in functions.iter().rev() {
        let (name, arg) = f
            .split_once('(')
            .ok_or_else(|| invalid("transform functions look like rotate(30deg)"))?;
        match (name.trim(), arg.trim()) {
            ("rotate", deg) => {
                let deg = deg
                    .strip_suffix("deg")
                    .and_then(|d| d.trim().parse::<f32>().ok())
                    .filter(|d| d.is_finite())
                    .ok_or_else(|| invalid("rotate takes an angle in deg"))?;
                t.rotate += deg;
            }
            ("scaleX", "-1") => {
                t.rotate = -t.rotate;
                t.flip_x = !t.flip_x;
            }
            ("scaleY", "-1") => {
                t.rotate = -t.rotate;
                t.flip_y = !t.flip_y;
            }
            _ => {
                return Err(invalid(
                    "transform supports rotate(<deg>), scaleX(-1) and scaleY(-1)",
                ));
            }
        }
    }
    if t.flip_x && t.flip_y {
        // Mirroring both ways is a half turn.
        t.flip_x = false;
        t.flip_y = false;
        t.rotate += 180.0;
    }
    t.rotate = t.rotate.rem_euclid(360.0);
    Ok(t)
}
/// Axis-aligned bounds of a rect turned about its center.
pub(crate) fn turned_bounds(r: Rect, turn: Turn) -> Rect {
    let (sin, cos) = turn.rotate.to_radians().sin_cos();
    let w = (r.width * cos).abs() + (r.height * sin).abs();
    let h = (r.width * sin).abs() + (r.height * cos).abs();
    Rect {
        x: r.x + (r.width - w) / 2.0,
        y: r.y + (r.height - h) / 2.0,
        width: w,
        height: h,
    }
}

/// Per-slide state shared while turning laid-out nodes into scene items.
struct Emitter<'a> {
    page: &'a str,
    tree: &'a TaffyTree<Leaf>,
    mapping: &'a BTreeMap<String, NodeId>,
    computed: &'a BTreeMap<String, TextStyle>,
    text: &'a mut TextEngine,
    resources: &'a Resources,
    items: Vec<Item>,
    diagnostics: &'a mut Vec<Diagnostic>,
}
impl Emitter<'_> {
    fn emit(&mut self, n: &Node, x: f32, y: f32, group: Option<&str>) -> Result<()> {
        let l = self
            .tree
            .layout(self.mapping[&n.id])
            .map_err(|e| invalid(e.to_string()))?;
        let rect = Rect {
            x: x + l.location.x,
            y: y + l.location.y,
            width: l.size.width,
            height: l.size.height,
        };
        let content = Rect {
            x: rect.x + l.border.left + l.padding.left,
            y: rect.y + l.border.top + l.padding.top,
            width: (rect.width - l.border.left - l.border.right - l.padding.left - l.padding.right)
                .max(0.0),
            height: (rect.height
                - l.border.top
                - l.border.bottom
                - l.padding.top
                - l.padding.bottom)
                .max(0.0),
        };
        let ts = self.computed[&n.id].clone();
        let mut item = Item {
            id: n.id.clone(),
            rect,
            group: group.map(str::to_owned),
            text_style: ts.clone(),
            source: Some(n.source.clone()),
            ..Item::default()
        };
        visual(n, &mut item)?;
        let fill = item.fill;
        let stroke_width = item.stroke_width;
        item.kind = match &n.kind {
            Kind::Box => Draw::Box,
            Kind::Group => Draw::Group,
            Kind::Text { paragraphs } => {
                item.rect = content;
                // Text padding/background is a separate painted box; text receives the content box.
                if fill.3 > 0 || stroke_width > 0.0 {
                    let mut bg = item.clone();
                    bg.rect = rect;
                    bg.id = format!("{}::background", n.id);
                    bg.kind = Draw::Box;
                    self.items.push(bg);
                    item.fill = Color(0, 0, 0, 0);
                    item.gradient = None;
                    item.stroke_width = 0.0;
                    item.shadow = None;
                }
                let wrap = n.style.get("white-space").is_none_or(|s| s != "nowrap");
                let mut shaped = self
                    .text
                    .shape(paragraphs, &ts, Some(content.width), wrap)?;
                let authored_lines = paragraphs.len()
                    + paragraphs
                        .iter()
                        .flat_map(|p| &p.spans)
                        .map(|s| s.text.chars().filter(|c| *c == '\n').count())
                        .sum::<usize>();
                // Intrinsic single-line labels must stay single-line in PPTX. Their
                // boxes fit our exact font advances; Office engines can differ slightly.
                // Explicit widths and genuinely wrapped paragraphs retain soft wrapping.
                let intrinsic_tight = n.style.get("width").is_none_or(|v| v == "auto")
                    && (content.width - shaped.width).abs() <= 0.5
                    && shaped.lines <= authored_lines;
                let export_wrap = wrap && !intrinsic_tight;
                if shaped.height > content.height + 0.5 || shaped.width > content.width + 0.5 {
                    self.diagnostics.push(diag(
                    "text_overflow",
                    Severity::Error,
                    self.page,
                    &item,
                    &format!("text measures {:.2}×{:.2}px ({} lines); available {:.2}×{:.2}px; font {} at {:.2}px",shaped.width,shaped.height,shaped.lines,content.width,content.height,ts.family,ts.size),
                ));
                }
                if export_wrap
                    && shaped.height <= content.height + 0.5
                    && self
                        .text
                        .shape(paragraphs, &ts, Some(content.width * 0.97), wrap)?
                        .height
                        > content.height + 0.5
                {
                    self.diagnostics.push(diag(
                        "text_tight",
                        Severity::Warning,
                        self.page,
                        &item,
                        "text has less than 3% width tolerance",
                    ));
                }
                let anchor: VerticalAlign = n
                    .style
                    .get("vertical-align")
                    .map(|v| keyword(v, "Text vertical-align"))
                    .transpose()?
                    .unwrap_or_default();
                let free = (content.height - shaped.height).max(0.0);
                let dy = match anchor {
                    VerticalAlign::Top => 0.0,
                    VerticalAlign::Middle => free / 2.0,
                    VerticalAlign::Bottom => free,
                };
                shaped.offset(content.x, content.y + dy);
                Draw::Text {
                    paragraphs: paragraphs.clone(),
                    glyphs: shaped.glyphs,
                    marks: shaped.marks,
                    line_count: shaped.lines,
                    export_wrap,
                    anchor,
                }
            }
            Kind::Image { src, clip } => {
                if !self.resources.images.contains_key(src) {
                    return Err(invalid(format!("missing image resource {src}")));
                }
                Draw::Image {
                    src: src.clone(),
                    fit: image_fit(&n.style)?,
                    clip: clip.clone(),
                }
            }
            Kind::Icon { src } => {
                let bytes = self
                    .resources
                    .images
                    .get(src)
                    .ok_or_else(|| invalid(format!("missing icon resource {src}")))?;
                Draw::Icon(crate::icon::parse_icon(bytes, item.rect)?)
            }
            Kind::Shape { shape } => Draw::Shape(shape.clone()),
            Kind::Path { data, view_box } => Draw::Path {
                data: data.clone(),
                view_box: *view_box,
            },
            Kind::Table { rows, columns } => {
                let cells =
                    table_cells(&item, rows, columns, self.text, self.page, self.diagnostics)?;
                Draw::Table {
                    rows: rows.clone(),
                    columns: columns.clone(),
                    cells,
                }
            }
            Kind::Chart {
                chart_type,
                labels,
                series,
            } => {
                let primitives = chart_items(
                    &item,
                    *chart_type,
                    labels,
                    series,
                    self.text,
                    self.page,
                    self.diagnostics,
                )?;
                Draw::Chart {
                    chart_type: *chart_type,
                    labels: labels.clone(),
                    series: series.clone(),
                    primitives,
                }
            }
        };
        self.items.push(item);
        let next_group = if matches!(n.kind, Kind::Group) {
            Some(n.id.as_str())
        } else {
            group
        };
        for c in &n.children {
            self.emit(c, rect.x, rect.y, next_group)
                .map_err(|e| at(&c.source, e))?;
        }
        Ok(())
    }
}
fn diag(code: &str, severity: Severity, page: &str, item: &Item, message: &str) -> Diagnostic {
    Diagnostic {
        code: code.into(),
        severity,
        page: page.into(),
        node_id: item.id.clone(),
        message: message.into(),
        source: item.source.clone(),
    }
}
fn primitive(parent: &Item, id: String, rect: Rect, fill: Color, kind: Draw) -> Item {
    Item {
        id,
        rect,
        fill,
        stroke: Color(0, 0, 0, 0),
        group: parent.group.clone(),
        kind,
        text_style: parent.text_style.clone(),
        source: parent.source.clone(),
        ..Item::default()
    }
}
fn label(
    parent: &Item,
    id: String,
    rect: Rect,
    value: &str,
    size: f32,
    color: Color,
    text: &mut TextEngine,
) -> Result<Item> {
    let paragraphs = vec![Paragraph::plain(value)];
    let mut s = parent.text_style.clone();
    s.size = size;
    s.line_height = size * 1.35;
    s.color = color;
    let mut shaped = text.shape(&paragraphs, &s, Some(rect.width), false)?;
    shaped.offset(rect.x, rect.y);
    let mut item = primitive(
        parent,
        id,
        rect,
        Color(0, 0, 0, 0),
        Draw::Text {
            paragraphs,
            glyphs: shaped.glyphs,
            marks: shaped.marks,
            line_count: shaped.lines,
            export_wrap: false,
            anchor: VerticalAlign::Top,
        },
    );
    item.text_style = s;
    Ok(item)
}
fn table_cells(
    parent: &Item,
    rows: &[Vec<String>],
    columns: &[f32],
    text: &mut TextEngine,
    page: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Vec<Item>> {
    let mut cells = Vec::new();
    let sum: f32 = columns.iter().sum();
    let rh = parent.rect.height / rows.len() as f32;
    for (r, row) in rows.iter().enumerate() {
        let mut x = parent.rect.x;
        for (c, value) in row.iter().enumerate() {
            let width = parent.rect.width * columns[c] / sum;
            let rect = Rect {
                x,
                y: parent.rect.y + r as f32 * rh,
                width,
                height: rh,
            };
            let mut cell = primitive(
                parent,
                format!("{}/cell/{r}/{c}", parent.id),
                rect,
                if r == 0 {
                    Color(235, 240, 248, 255)
                } else {
                    Color::WHITE
                },
                Draw::Box,
            );
            cell.stroke = Color(210, 217, 227, 255);
            cell.stroke_width = 1.0;
            cells.push(cell);
            let content = Rect {
                x: rect.x + 8.0,
                y: rect.y + 6.0,
                width: (width - 16.0).max(0.0),
                height: (rh - 12.0).max(0.0),
            };
            let paragraphs = vec![Paragraph::plain(value.clone())];
            let mut shape =
                text.shape(&paragraphs, &parent.text_style, Some(content.width), true)?;
            let mut ti = primitive(
                parent,
                format!("{}/text/{r}/{c}", parent.id),
                content,
                Color(0, 0, 0, 0),
                Draw::Box,
            );
            if shape.height > content.height + 0.5 {
                diagnostics.push(diag(
                    "table_text_overflow",
                    Severity::Error,
                    page,
                    &ti,
                    "cell text exceeds row height",
                ));
            }
            shape.offset(content.x, content.y);
            ti.kind = Draw::Text {
                paragraphs,
                glyphs: shape.glyphs,
                marks: shape.marks,
                line_count: shape.lines,
                export_wrap: true,
                anchor: VerticalAlign::Top,
            };
            cells.push(ti);
            x += width;
        }
    }
    Ok(cells)
}
fn chart_items(
    parent: &Item,
    kind: ChartType,
    labels: &[String],
    series: &[Series],
    text: &mut TextEngine,
    page: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Vec<Item>> {
    let mut out = Vec::new();
    let r = parent.rect;
    let plot = Rect {
        x: r.x + 64.0,
        y: r.y + 20.0,
        width: (r.width - 90.0).max(1.0),
        height: (r.height - 88.0).max(1.0),
    };
    let min = series
        .iter()
        .flat_map(|s| s.values.iter().copied())
        .fold(0.0f64, f64::min);
    let max = series
        .iter()
        .flat_map(|s| s.values.iter().copied())
        .fold(0.0f64, f64::max);
    let range = (max - min).max(1.0);
    let y = |v: f64| plot.bottom() - ((v - min) / range) as f32 * plot.height;
    for i in 0..=4 {
        let v = min + range * i as f64 / 4.0;
        let yy = y(v);
        out.push(primitive(
            parent,
            format!("{}/grid/{i}", parent.id),
            Rect {
                x: plot.x,
                y: yy,
                width: plot.width,
                height: 1.0,
            },
            Color(223, 229, 237, 255),
            Draw::Box,
        ));
        out.push(label(
            parent,
            format!("{}/tick/{i}", parent.id),
            Rect {
                x: r.x,
                y: yy - 10.0,
                width: 58.0,
                height: 24.0,
            },
            &format!("{v:.0}"),
            14.0,
            Color(90, 103, 122, 255),
            text,
        )?);
    }
    let slot = plot.width / labels.len() as f32;
    for (i, l) in labels.iter().enumerate() {
        out.push(label(
            parent,
            format!("{}/label/{i}", parent.id),
            Rect {
                x: plot.x + i as f32 * slot,
                y: plot.bottom() + 8.0,
                width: slot,
                height: 24.0,
            },
            l,
            14.0,
            Color(90, 103, 122, 255),
            text,
        )?);
    }
    for (si, s) in series.iter().enumerate() {
        let color = Color::parse(&s.color)?;
        if kind == ChartType::Bar {
            for (i, v) in s.values.iter().enumerate() {
                let bw = slot * 0.7 / series.len() as f32;
                let a = y(*v);
                let zero = y(0.0);
                out.push(primitive(
                    parent,
                    format!("{}/bar/{si}/{i}", parent.id),
                    Rect {
                        x: plot.x + i as f32 * slot + slot * 0.15 + si as f32 * bw,
                        y: a.min(zero),
                        width: (bw - 3.0).max(1.0),
                        height: (zero - a).abs(),
                    },
                    color,
                    Draw::Box,
                ));
            }
        } else {
            let mut d = String::new();
            for (i, v) in s.values.iter().enumerate() {
                d.push_str(&format!(
                    "{} {} {} ",
                    if i == 0 { "M" } else { "L" },
                    (i as f32 + 0.5) * slot,
                    (y(*v) - plot.y)
                ));
            }
            let mut path = primitive(
                parent,
                format!("{}/line/{si}", parent.id),
                plot,
                Color(0, 0, 0, 0),
                Draw::Path {
                    data: d,
                    view_box: [plot.width, plot.height],
                },
            );
            path.stroke = color;
            path.stroke_width = 3.0;
            out.push(path);
        }
        out.push(label(
            parent,
            format!("{}/legend/{si}", parent.id),
            Rect {
                x: plot.x + si as f32 * 160.0,
                y: r.bottom() - 28.0,
                width: 155.0,
                height: 26.0,
            },
            &s.name,
            14.0,
            color,
            text,
        )?);
    }
    // Labels never wrap, so a glyph outside its slot or the chart is clipped content.
    for item in &out {
        if let Draw::Text { glyphs, .. } = &item.kind
            && glyphs.iter().any(|g| {
                g.x < item.rect.x.max(r.x) - 0.5
                    || g.x + g.advance > item.rect.right().min(r.right()) + 0.5
            })
        {
            diagnostics.push(diag(
                "chart_label_overflow",
                Severity::Error,
                page,
                item,
                "chart tick, category label or legend does not fit inside the chart",
            ));
        }
    }
    Ok(out)
}
