//! Single-color SVG icons (such as Lucide or other outline icon sets) compiled to vector paths.
use crate::model::*;
use crate::render::Command;
use roxmltree::Node as X;

/// Full SVG path data as absolute M/L/C/Z; arcs, relative, smooth and quadratic segments
/// are converted.
pub(crate) fn path_commands(d: &str) -> Result<Vec<Command>> {
    let mut out = Vec::new();
    let mut pen = (0.0, 0.0);
    for segment in svgtypes::SimplifyingPathParser::from(d) {
        use svgtypes::SimplePathSegment::*;
        let segment = segment.map_err(|e| invalid(format!("invalid path data: {e}")))?;
        out.push(match segment {
            MoveTo { x, y } => {
                pen = (x, y);
                Command::M(x as f32, y as f32)
            }
            LineTo { x, y } => {
                pen = (x, y);
                Command::L(x as f32, y as f32)
            }
            CurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => {
                pen = (x, y);
                Command::C(
                    x1 as f32, y1 as f32, x2 as f32, y2 as f32, x as f32, y as f32,
                )
            }
            Quadratic { x1, y1, x, y } => {
                // Elevate to a cubic.
                let c1 = (
                    pen.0 + 2.0 / 3.0 * (x1 - pen.0),
                    pen.1 + 2.0 / 3.0 * (y1 - pen.1),
                );
                let c2 = (x + 2.0 / 3.0 * (x1 - x), y + 2.0 / 3.0 * (y1 - y));
                pen = (x, y);
                Command::C(
                    c1.0 as f32,
                    c1.1 as f32,
                    c2.0 as f32,
                    c2.1 as f32,
                    x as f32,
                    y as f32,
                )
            }
            ClosePath => Command::Z,
        });
        if out.len() > 20_000 {
            return Err(invalid("path data exceeds 20000 segments"));
        }
    }
    if out.is_empty() {
        return Err(invalid("path data is empty"));
    }
    Ok(out)
}

#[derive(Clone, Debug)]
pub struct IconPath {
    pub commands: Vec<Command>,
    pub fill: bool,
    pub stroke: bool,
}
/// An icon fitted into a frame (centered, aspect kept), in page units.
#[derive(Clone, Debug)]
pub struct Icon {
    pub paths: Vec<IconPath>,
    pub stroke_width: f32,
    /// Round caps and joins, as outline icon sets draw them.
    pub round: bool,
}

#[derive(Clone, Copy)]
struct Paint {
    fill: bool,
    stroke: bool,
    width: Option<f64>,
    round: Option<bool>,
}

pub(crate) fn parse_icon(bytes: &[u8], frame: Rect) -> Result<Icon> {
    if bytes.len() > 1024 * 1024 {
        return Err(invalid("icon SVG exceeds 1 MiB"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("icon SVG must be UTF-8"))?;
    if text.contains("<!DOCTYPE") || text.contains("<!ENTITY") {
        return Err(invalid("icon SVG may not use DTD or entities"));
    }
    let doc = roxmltree::Document::parse(text)?;
    let root = doc.root_element();
    if root.tag_name().name() != "svg" {
        return Err(invalid("an icon file's root must be <svg>"));
    }
    let numbers = |s: &str| {
        s.split([' ', ','])
            .filter(|v| !v.is_empty())
            .map(|v| v.parse::<f64>().ok())
            .collect::<Option<Vec<_>>>()
    };
    let view = root
        .attribute("viewBox")
        .and_then(numbers)
        .filter(|v| v.len() == 4 && v[2] > 0.0 && v[3] > 0.0)
        .or_else(|| {
            let size = |k| {
                root.attribute(k)?
                    .trim_end_matches("px")
                    .parse::<f64>()
                    .ok()
            };
            Some(vec![0.0, 0.0, size("width")?, size("height")?])
        })
        .ok_or_else(|| invalid("icon SVG needs a viewBox or width and height"))?;
    // Fit the viewBox inside the frame, centered, keeping its aspect ratio.
    let scale = (f64::from(frame.width) / view[2]).min(f64::from(frame.height) / view[3]);
    let ox =
        f64::from(frame.x) + (f64::from(frame.width) - view[2] * scale) / 2.0 - view[0] * scale;
    let oy =
        f64::from(frame.y) + (f64::from(frame.height) - view[3] * scale) / 2.0 - view[1] * scale;
    let place = |c: Command| {
        let p = |x: f32, y: f32| {
            (
                (f64::from(x) * scale + ox) as f32,
                (f64::from(y) * scale + oy) as f32,
            )
        };
        match c {
            Command::M(x, y) => {
                let (x, y) = p(x, y);
                Command::M(x, y)
            }
            Command::L(x, y) => {
                let (x, y) = p(x, y);
                Command::L(x, y)
            }
            Command::C(a, b, c, d, e, f) => {
                let ((a, b), (c, d), (e, f)) = (p(a, b), p(c, d), p(e, f));
                Command::C(a, b, c, d, e, f)
            }
            Command::Z => Command::Z,
        }
    };
    let mut paths = Vec::new();
    let mut widths = Vec::new();
    let mut round = None;
    let inherit = Paint {
        fill: true,
        stroke: false,
        width: None,
        round: None,
    };
    walk(root, inherit, &mut |n, paint| {
        let shape = shape_commands(n)?;
        if !paint.fill && !paint.stroke {
            return Ok(());
        }
        if paint.stroke {
            widths.push(paint.width.unwrap_or(1.0));
            round = round.or(paint.round);
        }
        paths.push(IconPath {
            commands: shape.into_iter().map(place).collect(),
            fill: paint.fill,
            stroke: paint.stroke,
        });
        Ok(())
    })?;
    if paths.is_empty() {
        return Err(invalid("icon SVG draws nothing"));
    }
    if widths.windows(2).any(|w| (w[0] - w[1]).abs() > 1e-6) {
        return Err(invalid("icon strokes must share one stroke-width"));
    }
    Ok(Icon {
        paths,
        stroke_width: (widths.first().copied().unwrap_or(0.0) * scale) as f32,
        round: round.unwrap_or(false),
    })
}

/// Visit drawable elements with their inherited paint.
fn walk(
    n: X<'_, '_>,
    parent: Paint,
    visit: &mut dyn FnMut(X<'_, '_>, Paint) -> Result<()>,
) -> Result<()> {
    if n.attribute("transform").is_some() {
        return Err(invalid("icon SVG transforms are not supported"));
    }
    let on = |k: &str, default: bool| n.attribute(k).map_or(default, |v| v.trim() != "none");
    let paint = Paint {
        fill: on("fill", parent.fill),
        stroke: on("stroke", parent.stroke),
        width: n
            .attribute("stroke-width")
            .and_then(|v| v.trim_end_matches("px").parse().ok())
            .or(parent.width),
        round: n
            .attribute("stroke-linecap")
            .map(|v| v == "round")
            .or(parent.round),
    };
    for child in n.children().filter(|c| c.is_element()) {
        match child.tag_name().name() {
            "g" => walk(child, paint, visit)?,
            "path" | "circle" | "ellipse" | "rect" | "line" | "polyline" | "polygon" => {
                if child.attribute("transform").is_some() {
                    return Err(invalid("icon SVG transforms are not supported"));
                }
                let mut own = paint;
                own.fill = child
                    .attribute("fill")
                    .map_or(own.fill, |v| v.trim() != "none");
                own.stroke = child
                    .attribute("stroke")
                    .map_or(own.stroke, |v| v.trim() != "none");
                own.width = child
                    .attribute("stroke-width")
                    .and_then(|v| v.trim_end_matches("px").parse().ok())
                    .or(own.width);
                if child.tag_name().name() == "line" || child.tag_name().name() == "polyline" {
                    own.fill = false;
                }
                visit(child, own)?;
            }
            "title" | "desc" | "metadata" => {}
            other => {
                return Err(invalid(format!(
                    "icon SVG element <{other}> is not supported"
                )));
            }
        }
    }
    Ok(())
}

/// One drawable element as path commands in viewBox units.
fn shape_commands(n: X<'_, '_>) -> Result<Vec<Command>> {
    let num = |k: &str| -> Result<f64> {
        n.attribute(k)
            .unwrap_or("0")
            .trim_end_matches("px")
            .parse::<f64>()
            .map_err(|_| invalid(format!("invalid {k} in icon SVG")))
    };
    let d = match n.tag_name().name() {
        "path" => n.attribute("d").unwrap_or("").to_owned(),
        "circle" | "ellipse" => {
            let (cx, cy) = (num("cx")?, num("cy")?);
            let (rx, ry) = if n.tag_name().name() == "circle" {
                (num("r")?, num("r")?)
            } else {
                (num("rx")?, num("ry")?)
            };
            format!(
                "M {} {cy} A {rx} {ry} 0 1 0 {} {cy} A {rx} {ry} 0 1 0 {} {cy} Z",
                cx - rx,
                cx + rx,
                cx - rx
            )
        }
        "rect" => {
            let (x, y, w, h) = (num("x")?, num("y")?, num("width")?, num("height")?);
            let r = num("rx")?.max(num("ry")?).min(w / 2.0).min(h / 2.0);
            if r > 0.0 {
                format!(
                    "M {} {y} H {} A {r} {r} 0 0 1 {} {} V {} A {r} {r} 0 0 1 {} {} H {} A {r} {r} 0 0 1 {x} {} V {} A {r} {r} 0 0 1 {} {y} Z",
                    x + r,
                    x + w - r,
                    x + w,
                    y + r,
                    y + h - r,
                    x + w - r,
                    y + h,
                    x + r,
                    y + h - r,
                    y + r,
                    x + r
                )
            } else {
                format!("M {x} {y} H {} V {} H {x} Z", x + w, y + h)
            }
        }
        "line" => format!(
            "M {} {} L {} {}",
            num("x1")?,
            num("y1")?,
            num("x2")?,
            num("y2")?
        ),
        tag @ ("polyline" | "polygon") => {
            let points = n.attribute("points").unwrap_or("").trim();
            format!("M {points}{}", if tag == "polygon" { " Z" } else { "" })
        }
        _ => unreachable!(),
    };
    path_commands(&d)
}
