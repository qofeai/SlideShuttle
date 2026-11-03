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
                            Kind::Text { spans } => {
                                match text.shape(spans, &leaf.text, width, leaf.wrap) {
                                    Ok(s) => (s.width, s.height),
                                    Err(_) => (0.0, leaf.text.line_height),
                                }
                            }
                            Kind::Image { src } => resources
                                .images
                                .get(src)
                                .and_then(|b| crate::render::decode_image(b).ok())
                                .map(|im| (im.width() as f32, im.height() as f32))
                                .unwrap_or((100.0, 100.0)),
                            Kind::Table { rows, .. } => (
                                width.unwrap_or(600.0),
                                rows.len() as f32 * (leaf.text.line_height + 20.0),
                            ),
                            Kind::Chart { .. } => (width.unwrap_or(600.0), 320.0),
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
        let mut page = ScenePage {
            id: slide.id.clone(),
            background: Color::parse(&slide.background)?,
            items: Vec::new(),
        };
        for node in &slide.nodes {
            emit(
                node,
                0.0,
                0.0,
                None,
                &slide.id,
                &tree,
                &mapping,
                &computed,
                text,
                resources,
                &mut page.items,
                &mut scene.diagnostics,
            )
            .map_err(|e| at(&node.source, e))?;
        }
        for item in &page.items {
            if item.rect.x < -0.5
                || item.rect.y < -0.5
                || item.rect.right() > deck.width + 0.5
                || item.rect.bottom() > deck.height + 0.5
            {
                scene.diagnostics.push(diag(
                    "out_of_bounds",
                    "error",
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
    let style = box_style(&n.style)?;
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
fn visual(n: &Node) -> Result<(Color, Option<LinearGradient>, Color, f32, f32, f32)> {
    let value = n
        .style
        .get("background")
        .map(String::as_str)
        .unwrap_or("transparent");
    let gradient = if let Some(v) = value
        .strip_prefix("linear-gradient(")
        .and_then(|v| v.strip_suffix(')'))
    {
        let v = v.split(',').map(str::trim).collect::<Vec<_>>();
        if v.len() != 3 || !["to right", "to bottom"].contains(&v[0]) {
            return Err(invalid(
                "gradient supports: linear-gradient(to right|to bottom, #color, #color)",
            ));
        }
        let start = Color::parse(v[1])?;
        let end = Color::parse(v[2])?;
        if start.3 != 255 || end.3 != 255 {
            return Err(invalid("gradient stops must be opaque"));
        }
        Some(LinearGradient {
            vertical: v[0] == "to bottom",
            start,
            end,
        })
    } else {
        None
    };
    let fill = if let Some(g) = gradient {
        g.start
    } else {
        Color::parse(value)?
    };
    let stroke = n
        .style
        .get("border-color")
        .map(|v| Color::parse(v))
        .transpose()?
        .unwrap_or(Color::BLACK);
    let width = n
        .style
        .get("border-width")
        .map(|v| length_px(v))
        .transpose()?
        .unwrap_or(0.0);
    let radius = n
        .style
        .get("border-radius")
        .map(|v| length_px(v))
        .transpose()?
        .unwrap_or(0.0);
    let opacity = n
        .style
        .get("opacity")
        .map(|v| nonnegative(v))
        .transpose()?
        .unwrap_or(1.0);
    if opacity > 1.0 || radius < 0.0 || width < 0.0 {
        return Err(invalid("invalid opacity, radius or border width"));
    }
    if !n.children.is_empty() && opacity != 1.0 {
        return Err(invalid(
            "container opacity is not supported; apply opacity to leaves",
        ));
    }
    if matches!(n.kind, Kind::Image { .. }) && (radius > 0.0 || width > 0.0 || fill.3 > 0) {
        return Err(invalid(
            "image decoration belongs on a wrapping Box; Image border, background and radius are not supported",
        ));
    }
    if matches!(n.kind, Kind::Table { .. } | Kind::Chart { .. })
        && (gradient.is_some() || opacity != 1.0)
    {
        return Err(invalid("table/chart gradient or opacity is not supported"));
    }
    Ok((fill, gradient, stroke, width, radius, opacity))
}

#[allow(clippy::too_many_arguments)]
fn emit(
    n: &Node,
    x: f32,
    y: f32,
    group: Option<&str>,
    page: &str,
    tree: &TaffyTree<Leaf>,
    mapping: &BTreeMap<String, NodeId>,
    computed: &BTreeMap<String, TextStyle>,
    text: &mut TextEngine,
    resources: &Resources,
    items: &mut Vec<Item>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<()> {
    let l = tree
        .layout(mapping[&n.id])
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
        height: (rect.height - l.border.top - l.border.bottom - l.padding.top - l.padding.bottom)
            .max(0.0),
    };
    let (fill, gradient, stroke, stroke_width, radius, opacity) = visual(n)?;
    let ts = computed[&n.id].clone();
    let mut item = Item {
        id: n.id.clone(),
        rect,
        fill,
        gradient,
        stroke,
        stroke_width,
        radius,
        opacity,
        group: group.map(str::to_owned),
        kind: Draw::Box,
        text_style: ts.clone(),
        source: Some(n.source.clone()),
    };
    item.kind = match &n.kind {
        Kind::Box => Draw::Box,
        Kind::Group => Draw::Group,
        Kind::Text { spans } => {
            item.rect = content;
            // Text padding/background is a separate painted box; text receives the content box.
            if fill.3 > 0 || stroke_width > 0.0 {
                let mut bg = item.clone();
                bg.rect = rect;
                bg.id = format!("{}::background", n.id);
                bg.kind = Draw::Box;
                items.push(bg);
                item.fill = Color(0, 0, 0, 0);
                item.gradient = None;
                item.stroke_width = 0.0;
            }
            let wrap = n.style.get("white-space").is_none_or(|s| s != "nowrap");
            let mut shaped = text.shape(spans, &ts, Some(content.width), wrap)?;
            let authored_lines = 1 + spans
                .iter()
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
                diagnostics.push(diag(
                    "text_overflow",
                    "error",
                    page,
                    &item,
                    &format!("text measures {:.2}×{:.2}px ({} lines); available {:.2}×{:.2}px; font {} at {:.2}px",shaped.width,shaped.height,shaped.lines,content.width,content.height,ts.family,ts.size),
                ));
            }
            let stress = text.shape(spans, &ts, Some(content.width * 0.97), wrap)?;
            if export_wrap
                && stress.height > content.height + 0.5
                && shaped.height <= content.height + 0.5
            {
                diagnostics.push(diag(
                    "text_tight",
                    "warning",
                    page,
                    &item,
                    "text has less than 3% width tolerance",
                ));
            }
            for g in &mut shaped.glyphs {
                g.x += content.x;
                g.y += content.y;
            }
            Draw::Text {
                spans: spans.clone(),
                glyphs: shaped.glyphs,
                line_count: shaped.lines,
                export_wrap,
            }
        }
        Kind::Image { src } => {
            if !resources.images.contains_key(src) {
                return Err(invalid(format!("missing image resource {src}")));
            }
            Draw::Image {
                src: src.clone(),
                fit: n
                    .style
                    .get("object-fit")
                    .cloned()
                    .unwrap_or("contain".into()),
            }
        }
        Kind::Shape { shape } => Draw::Shape(shape.clone()),
        Kind::Path { data, view_box } => Draw::Path {
            data: data.clone(),
            view_box: *view_box,
        },
        Kind::Table { rows, columns } => {
            let cells = table_cells(&item, rows, columns, text, page, diagnostics)?;
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
            let primitives =
                chart_items(&item, chart_type, labels, series, text, page, diagnostics)?;
            Draw::Chart {
                chart_type: chart_type.clone(),
                labels: labels.clone(),
                series: series.clone(),
                primitives,
            }
        }
    };
    items.push(item);
    let next_group = if matches!(n.kind, Kind::Group) {
        Some(n.id.as_str())
    } else {
        group
    };
    for c in &n.children {
        emit(
            c,
            rect.x,
            rect.y,
            next_group,
            page,
            tree,
            mapping,
            computed,
            text,
            resources,
            items,
            diagnostics,
        )
        .map_err(|e| at(&c.source, e))?;
    }
    Ok(())
}
fn diag(code: &str, severity: &str, page: &str, item: &Item, message: &str) -> Diagnostic {
    Diagnostic {
        code: code.into(),
        severity: severity.into(),
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
        gradient: None,
        stroke: Color(0, 0, 0, 0),
        stroke_width: 0.0,
        radius: 0.0,
        opacity: 1.0,
        group: parent.group.clone(),
        kind,
        text_style: parent.text_style.clone(),
        source: parent.source.clone(),
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
    let spans = vec![Span {
        text: value.into(),
        style: Css::new(),
    }];
    let mut s = parent.text_style.clone();
    s.size = size;
    s.line_height = size * 1.35;
    s.color = color;
    let mut shaped = text.shape(&spans, &s, Some(rect.width), false)?;
    for g in &mut shaped.glyphs {
        g.x += rect.x;
        g.y += rect.y;
    }
    let mut item = primitive(
        parent,
        id,
        rect,
        Color(0, 0, 0, 0),
        Draw::Text {
            spans,
            glyphs: shaped.glyphs,
            line_count: shaped.lines,
            export_wrap: false,
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
            let spans = vec![Span {
                text: value.clone(),
                style: Css::new(),
            }];
            let mut shape = text.shape(&spans, &parent.text_style, Some(content.width), true)?;
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
                    "error",
                    page,
                    &ti,
                    "cell text exceeds row height",
                ));
            }
            for g in &mut shape.glyphs {
                g.x += content.x;
                g.y += content.y;
            }
            ti.kind = Draw::Text {
                spans,
                glyphs: shape.glyphs,
                line_count: shape.lines,
                export_wrap: true,
            };
            cells.push(ti);
            x += width;
        }
    }
    Ok(cells)
}
fn chart_items(
    parent: &Item,
    kind: &str,
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
        if kind == "bar" {
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
                "error",
                page,
                item,
                "chart tick, category label or legend does not fit inside the chart",
            ));
        }
    }
    Ok(out)
}
