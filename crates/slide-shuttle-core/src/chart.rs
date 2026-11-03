//! Data charts: number formats, axes, the PNG/PDF drawing and the native Office chart part.
//! The Office part pins the same plot, legend and title boxes and the same axis ticks,
//! so PowerPoint draws the chart where the preview does.
use crate::model::*;
use crate::pptx::{Package, fill};
use crate::text::TextEngine;

/// Series colors, in order, when neither the Series nor `chart-colors` sets one.
pub(crate) const PALETTE: [Color; 8] = [
    Color(0x25, 0x63, 0xEB, 255),
    Color(0x16, 0xA3, 0x4A, 255),
    Color(0xF5, 0x9E, 0x0B, 255),
    Color(0xDC, 0x26, 0x26, 255),
    Color(0x7C, 0x3A, 0xED, 255),
    Color(0x08, 0x91, 0xB2, 255),
    Color(0xDB, 0x27, 0x77, 255),
    Color(0x65, 0xA3, 0x0D, 255),
];
/// Office gapWidth: the space between bar clusters, as a share of one bar.
const GAP: f32 = 0.8;
const HOLE: f32 = 0.5;
const LINE_WIDTH: f32 = 3.0;
const MARKER: f32 = 8.0;
/// White edge between pie slices.
const SEPARATOR: f32 = 2.0;
const C: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

struct Format {
    prefix: String,
    suffix: String,
    /// Decimals always shown ("0") and at most shown ("0" and "#").
    min: usize,
    max: usize,
    thousands: bool,
    percent: bool,
}

/// A subset of Excel number formats: literal prefix and suffix, `#,##0`, decimals and `%`.
fn parse_format(code: &str) -> Result<Option<Format>> {
    if code.eq_ignore_ascii_case("general") {
        return Ok(None);
    }
    let unsupported = || invalid(format!("unsupported number format {code}"));
    let (mut prefix, mut suffix, mut digits) = (String::new(), String::new(), String::new());
    let mut percent = false;
    let mut chars = code.chars();
    while let Some(c) = chars.next() {
        if matches!(c, '0' | '#' | ',' | '.') && suffix.is_empty() {
            digits.push(c);
            continue;
        }
        let literal = if digits.is_empty() {
            &mut prefix
        } else {
            &mut suffix
        };
        match c {
            '"' => loop {
                match chars.next() {
                    Some('"') => break,
                    Some(ch) => literal.push(ch),
                    None => return Err(unsupported()),
                }
            },
            '\\' => literal.push(chars.next().ok_or_else(unsupported)?),
            '%' => {
                percent = true;
                literal.push('%');
            }
            c if c.is_ascii_alphanumeric() || ";[@*_?/".contains(c) => return Err(unsupported()),
            c => literal.push(c),
        }
    }
    let (int, frac) = digits.split_once('.').unwrap_or((&digits, ""));
    if !int.contains(['0', '#']) && frac.is_empty() || int.ends_with(',') || frac.contains(',') {
        return Err(unsupported());
    }
    Ok(Some(Format {
        prefix,
        suffix,
        min: frac.chars().filter(|c| *c == '0').count(),
        max: frac.len(),
        thousands: int.contains(','),
        percent,
    }))
}

/// Excel's General format: integers as is, otherwise up to ten significant digits.
fn general(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let places = (9 - v.abs().log10().floor() as i32).clamp(0, 15) as usize;
    let s = format!("{v:.places$}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

pub(crate) fn format_value(v: f64, code: &str) -> Result<String> {
    let Some(f) = parse_format(code)? else {
        return Ok(general(v));
    };
    let v = if f.percent { v * 100.0 } else { v };
    let mut s = format!("{:.*}", f.max, v.abs());
    if f.max > f.min {
        let keep = s.len() - (f.max - f.min);
        while s.len() > keep && s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if f.thousands {
        let end = s.find('.').unwrap_or(s.len());
        let mut at = end;
        while at > 3 {
            at -= 3;
            s.insert(at, ',');
        }
    }
    let sign = if v < 0.0 && s.chars().any(|c| ('1'..='9').contains(&c)) {
        "-"
    } else {
        ""
    };
    Ok(format!("{sign}{}{s}{}", f.prefix, f.suffix))
}

/// The smallest of 1, 2, 2.5, 5 × 10ⁿ at or above `x`.
fn nice(x: f64) -> f64 {
    let e = 10f64.powf(x.log10().floor());
    [1.0, 2.0, 2.5, 5.0, 10.0]
        .into_iter()
        .map(|m| m * e)
        .find(|v| *v >= x * (1.0 - 1e-9))
        .unwrap_or(10.0 * e)
}

/// An axis over the data range: given bounds and step win; otherwise round bounds and
/// the finest round step whose ticks are at least `needed` px apart.
fn axis(
    (lo, hi): (f64, f64),
    (min, max, step): (Option<f64>, Option<f64>, Option<f64>),
    length: f32,
    needed: &mut dyn FnMut(&Axis) -> Result<f32>,
) -> Result<Axis> {
    let (lo, hi) = (min.unwrap_or(lo), max.unwrap_or(hi));
    let hi = if hi > lo { hi } else { lo + lo.abs().max(1.0) };
    let fit = |step: f64| Axis {
        min: min.unwrap_or(((lo / step) + 1e-9).floor() * step),
        max: max.unwrap_or(((hi / step) - 1e-9).ceil() * step),
        step,
    };
    if let Some(step) = step {
        let a = fit(step);
        if (a.max - a.min) / step > 200.0 {
            return Err(invalid("Chart step gives more than 200 ticks"));
        }
        return Ok(a);
    }
    let mut step = nice((hi - lo) / 12.0);
    loop {
        let a = fit(step);
        let spacing = length * (step / (a.max - a.min)) as f32;
        if spacing >= needed(&a)? || a.max - a.min <= step * 2.0 + 1e-9 {
            return Ok(a);
        }
        step = nice(step * 1.0001);
    }
}
fn ticks(a: &Axis) -> Vec<f64> {
    (0..)
        .map(|i| a.min + i as f64 * a.step)
        .take_while(|v| *v <= a.max + a.step * 1e-9)
        .collect()
}

/// Black or white, whichever reads better on `bg`.
fn ink(bg: Color) -> Color {
    let l = 0.2126 * f32::from(bg.0) + 0.7152 * f32::from(bg.1) + 0.0722 * f32::from(bg.2);
    if l > 150.0 {
        Color(0x1F, 0x29, 0x37, 255)
    } else {
        Color(255, 255, 255, 255)
    }
}
fn faded(c: Color, alpha: f32) -> Color {
    Color(c.0, c.1, c.2, (f32::from(c.3) * alpha).round() as u8)
}

/// Per-category share of the stacked series, for percent stacking.
fn shares(chart: &Chart) -> Vec<f64> {
    (0..chart.categories.len())
        .map(|i| chart.series.iter().map(|s| s.values[i].abs()).sum::<f64>())
        .collect()
}
fn stacked(chart: &Chart, s: &Series) -> bool {
    chart.stack != Stack::None && s.kind == chart.kind
}
/// Value at point `i` as plotted: a share of the category total under percent stacking.
fn plotted(chart: &Chart, totals: &[f64], s: &Series, i: usize) -> f64 {
    if chart.stack == Stack::Percent && totals[i] > 0.0 {
        s.values[i] / totals[i]
    } else {
        s.values[i]
    }
}

struct Painter<'a> {
    parent: &'a Item,
    style: TextStyle,
    items: Vec<Item>,
    /// Labels already reported as not fitting.
    reported: Vec<String>,
}
impl Painter<'_> {
    fn part(&self, id: String, rect: Rect, kind: Draw) -> Item {
        Item {
            id: format!("{}/{id}", self.parent.id),
            rect,
            group: self.parent.group.clone(),
            kind,
            text_style: self.style.clone(),
            source: self.parent.source.clone(),
            stroke: Color::default(),
            ..Item::default()
        }
    }
    fn measure(&self, text: &mut TextEngine, value: &str, style: &TextStyle) -> Result<f32> {
        Ok(text
            .shape(&[Paragraph::plain(value)], style, None, false)?
            .width)
    }
    /// A one-line label; `anchor` is the point's place in the label box (0–1 each way).
    fn label(
        &mut self,
        text: &mut TextEngine,
        id: String,
        value: &str,
        style: &TextStyle,
        (x, y): (f32, f32),
        (ax, ay): (f32, f32),
    ) -> Result<Rect> {
        let paragraphs = vec![Paragraph::plain(value)];
        let mut shaped = text.shape(&paragraphs, style, None, false)?;
        let rect = Rect {
            x: x - ax * shaped.width,
            y: y - ay * style.line_height,
            width: shaped.width,
            height: style.line_height,
        };
        shaped.offset(rect.x, rect.y + (style.line_height - shaped.height) / 2.0);
        let mut item = self.part(
            id,
            rect,
            Draw::Text {
                paragraphs,
                glyphs: shaped.glyphs,
                marks: shaped.marks,
                line_count: shaped.lines,
                export_wrap: false,
                anchor: VerticalAlign::Top,
            },
        );
        item.text_style = style.clone();
        self.items.push(item);
        Ok(rect)
    }
    fn fill(&mut self, id: String, rect: Rect, color: Color) {
        let mut item = self.part(id, rect, Draw::Box);
        item.fill = color;
        self.items.push(item);
    }
    fn line(&mut self, id: String, (x1, y1): (f32, f32), (x2, y2): (f32, f32), color: Color) {
        let mut item = self.part(
            id,
            Rect {
                x: x1,
                y: y1,
                width: x2 - x1,
                height: y2 - y1,
            },
            Draw::Shape(ShapeKind {
                preset: "line".into(),
                adjust: Vec::new(),
            }),
        );
        item.stroke = color;
        item.stroke_width = 1.0;
        self.items.push(item);
    }
    /// Path data in page coordinates, drawn inside `frame`.
    /// Path data relative to `frame`'s origin, in px.
    fn path(&mut self, id: String, frame: Rect, data: String) -> &mut Item {
        let item = self.part(
            id,
            frame,
            Draw::Path {
                data,
                view_box: [frame.width, frame.height],
            },
        );
        self.items.push(item);
        self.items.last_mut().unwrap()
    }
    fn marker(&mut self, id: String, (x, y): (f32, f32), color: Color) {
        let mut item = self.part(
            id,
            Rect {
                x: x - MARKER / 2.0,
                y: y - MARKER / 2.0,
                width: MARKER,
                height: MARKER,
            },
            Draw::Shape(ShapeKind {
                preset: "ellipse".into(),
                adjust: Vec::new(),
            }),
        );
        item.fill = color;
        self.items.push(item);
    }
}

/// Lay the chart out inside its box and resolve its colors: what PNG/PDF paint plus the
/// boxes and axes the native chart pins.
pub(crate) fn place(
    item: &Item,
    chart: &Chart,
    palette: &[Color],
    text: &mut TextEngine,
    diagnostic: &mut dyn FnMut(&str, String),
) -> Result<Draw> {
    let mut chart = chart.clone();
    let round = chart.kind.is_round();
    for (i, s) in chart.series.iter_mut().enumerate() {
        if s.colors.is_empty() {
            s.colors = if round {
                (0..s.values.len())
                    .map(|j| palette[j % palette.len()])
                    .collect()
            } else {
                vec![palette[i % palette.len()]]
            };
        }
    }
    let r = item.rect;
    let mut style = item.text_style.clone();
    style.line_height = (style.size * 1.3).round().max(1.0);
    style.align = Align::Left;
    let (size, lh) = (style.size, style.line_height);
    let gap = (size * 0.4).round();
    let pad = (size * 0.5).round().max(4.0);
    let grid_color = faded(style.color, 0.15);
    let axis_color = faded(style.color, 0.35);
    let mut p = Painter {
        parent: item,
        style: style.clone(),
        items: Vec::new(),
        reported: Vec::new(),
    };
    let mut area = Rect {
        x: r.x + pad,
        y: r.y + pad,
        width: (r.width - 2.0 * pad).max(1.0),
        height: (r.height - 2.0 * pad).max(1.0),
    };
    let title = match &chart.title {
        Some(t) => {
            let mut ts = style.clone();
            ts.size = (size * 1.2).round();
            ts.line_height = (ts.size * 1.3).round();
            ts.weight = 700;
            let rect = p.label(
                text,
                "title".into(),
                t,
                &ts,
                (area.x + area.width / 2.0, area.y),
                (0.5, 0.0),
            )?;
            area.y += ts.line_height + pad;
            area.height -= ts.line_height + pad;
            Some(rect)
        }
        None => None,
    };
    let legend = match chart.legend {
        Some(side) => Some(legend(&mut p, text, &chart, side, &mut area, pad)?),
        None => None,
    };
    let (mut axis_v, mut axis_x) = (None, None);
    let plot = if round {
        let d = area.width.min(area.height);
        let plot = Rect {
            x: area.x + (area.width - d) / 2.0,
            y: area.y + (area.height - d) / 2.0,
            width: d,
            height: d,
        };
        pie(&mut p, text, &chart, plot)?;
        plot
    } else {
        let totals = shares(&chart);
        let mut extent = (0.0f64, 0.0f64);
        if chart.kind == ChartType::Scatter {
            extent = (f64::INFINITY, f64::NEG_INFINITY);
        }
        for i in 0..chart.categories.len() {
            let (mut pos, mut neg) = (0.0, 0.0);
            for s in &chart.series {
                let v = plotted(&chart, &totals, s, i);
                if stacked(&chart, s) {
                    if v >= 0.0 {
                        pos += v
                    } else {
                        neg += v
                    }
                    extent = (extent.0.min(neg), extent.1.max(pos));
                } else {
                    extent = (extent.0.min(v), extent.1.max(v));
                }
            }
        }
        let bounds = if chart.stack == Stack::Percent {
            (Some(0.0), Some(1.0), None)
        } else {
            (chart.min, chart.max, chart.step)
        };
        let format = if chart.stack == Stack::Percent {
            "0%"
        } else {
            chart.format.as_str()
        }
        .to_owned();
        let tick_label = |v: f64| format_value(v, &format);
        let horizontal = chart.horizontal;
        let bottom = area.bottom() - lh - gap;
        let mut plot = if horizontal {
            let mut widest = 0.0f32;
            for c in &chart.categories {
                widest = widest.max(p.measure(text, c, &style)?);
            }
            let x = area.x + widest + gap;
            Rect {
                x,
                y: area.y,
                width: (area.right() - x).max(1.0),
                height: (bottom - area.y).max(1.0),
            }
        } else {
            let y = area.y + lh / 2.0;
            Rect {
                x: area.x,
                y,
                width: area.width,
                height: (bottom - y).max(1.0),
            }
        };
        // Horizontal ticks need room for their widest label; vertical ones for a line of text.
        let spread = |a: &Axis, p: &Painter<'_>, text: &mut TextEngine| -> Result<f32> {
            let mut widest = 0.0f32;
            for t in ticks(a) {
                widest = widest.max(p.measure(text, &tick_label(t)?, &style)?);
            }
            Ok(widest)
        };
        let a = if horizontal {
            let est = plot.width - size * 2.0;
            axis(extent, bounds, est, &mut |a| {
                Ok(spread(a, &p, text)? * 1.15 + size * 1.5)
            })?
        } else {
            axis(extent, bounds, plot.height, &mut |_| Ok(lh * 2.5))?
        };
        let widest = spread(&a, &p, text)?;
        if horizontal {
            let last = p.measure(text, &tick_label(*ticks(&a).last().unwrap())?, &style)?;
            plot.width = (area.right() - last / 2.0 - plot.x).max(1.0);
        } else {
            plot.x = area.x + widest + gap;
            plot.width = (area.right() - plot.x).max(1.0);
        }
        if chart.kind == ChartType::Scatter {
            let xs = chart
                .categories
                .iter()
                .map(|c| c.parse::<f64>().unwrap())
                .collect::<Vec<_>>();
            let range = xs
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
                    (lo.min(*v), hi.max(*v))
                });
            let est = plot.width - size * 2.0;
            let mut need = |a: &Axis| -> Result<f32> {
                let mut w = 0.0f32;
                for t in ticks(a) {
                    w = w.max(p.measure(text, &general(t), &style)?);
                }
                Ok(w * 1.15 + size * 1.5)
            };
            let x = axis(range, (None, None, None), est, &mut need)?;
            let last = p.measure(text, &general(*ticks(&x).last().unwrap()), &style)?;
            plot.width = (area.right() - last / 2.0 - plot.x).max(1.0);
            axis_x = Some(x);
        }
        cartesian(
            &mut p,
            text,
            &chart,
            plot,
            &a,
            axis_x.as_ref(),
            &format,
            grid_color,
            axis_color,
            gap,
            diagnostic,
        )?;
        axis_v = Some(a);
        plot
    };
    // Labels never wrap, so a glyph outside the chart is clipped content.
    for part in &p.items {
        if let Draw::Text {
            glyphs, paragraphs, ..
        } = &part.kind
            && !p.reported.contains(&part.id)
            && glyphs
                .iter()
                .any(|g| g.x < r.x - 0.5 || g.x + g.advance > r.right() + 0.5)
        {
            let words = paragraphs
                .iter()
                .flat_map(|q| &q.spans)
                .map(|s| s.text.as_str())
                .collect::<String>();
            diagnostic(
                "chart_label_overflow",
                format!("chart text {words:?} does not fit inside the chart"),
            );
        }
    }
    Ok(Draw::Chart {
        chart: Box::new(chart),
        axis: axis_v,
        x_axis: axis_x,
        plot,
        legend,
        title,
        primitives: p.items,
    })
}

/// Legend entries in rows (top/bottom) or a column (left/right); returns the legend box and
/// takes its room from `area`.
fn legend(
    p: &mut Painter<'_>,
    text: &mut TextEngine,
    chart: &Chart,
    side: Side,
    area: &mut Rect,
    pad: f32,
) -> Result<Rect> {
    let style = p.style.clone();
    // Office spaces legend entries wider than a text line and hides entries that overflow.
    let (size, lh) = (style.size, (style.size * 1.8).round());
    let key = (size * 0.7).round();
    let between = (size * 1.2).round();
    let mut entries: Vec<(&str, Color, bool)> = if chart.kind.is_round() {
        let s = &chart.series[0];
        chart
            .categories
            .iter()
            .zip(&s.colors)
            .map(|(c, k)| (c.as_str(), *k, false))
            .collect()
    } else {
        chart
            .series
            .iter()
            .map(|s| (s.name.as_str(), s.colors[0], s.kind == ChartType::Line))
            .collect()
    };
    // A side legend lists stacked columns top-down, as they are stacked; Office does the same.
    if matches!(side, Side::Left | Side::Right) && chart.stack != Stack::None && !chart.horizontal {
        entries.reverse();
    }
    let mut widths = Vec::new();
    for (name, _, line) in &entries {
        let k = if *line { key * 2.0 } else { key };
        widths.push((k, k + size * 0.4 + p.measure(text, name, &style)?));
    }
    // (entry, x offset, row) inside the legend box.
    let mut spots = Vec::new();
    let (w, h) = if matches!(side, Side::Top | Side::Bottom) {
        let mut rows: Vec<f32> = vec![0.0];
        for (i, (_, width)) in widths.iter().enumerate() {
            let last = rows.len() - 1;
            if rows[last] > 0.0 && rows[last] + between + width > area.width {
                rows.push(0.0);
            }
            let last = rows.len() - 1;
            if rows[last] > 0.0 {
                rows[last] += between;
            }
            spots.push((i, rows[last], last));
            rows[last] += width;
        }
        let w = rows.iter().fold(0.0f32, |a, b| a.max(*b));
        // Center each row inside the legend box.
        for (_, x, row) in &mut spots {
            *x += (w - rows[*row]) / 2.0;
        }
        (w, rows.len() as f32 * lh)
    } else {
        for i in 0..widths.len() {
            spots.push((i, 0.0, i));
        }
        (
            widths.iter().fold(0.0f32, |a, (_, b)| a.max(*b)),
            widths.len() as f32 * lh,
        )
    };
    let rect = match side {
        Side::Top | Side::Bottom => {
            let y = if side == Side::Top {
                area.y
            } else {
                area.bottom() - h
            };
            if side == Side::Top {
                area.y += h + pad;
            }
            area.height -= h + pad;
            Rect {
                x: area.x + (area.width - w) / 2.0,
                y,
                width: w,
                height: h,
            }
        }
        Side::Left | Side::Right => {
            let x = if side == Side::Left {
                area.x
            } else {
                area.right() - w
            };
            if side == Side::Left {
                area.x += w + pad;
            }
            area.width -= w + pad;
            Rect {
                x,
                y: area.y + (area.height - h) / 2.0,
                width: w,
                height: h,
            }
        }
    };
    for (i, x, row) in spots {
        let (name, color, line) = entries[i];
        let (kw, _) = widths[i];
        let (x, cy) = (rect.x + x, rect.y + row as f32 * lh + lh / 2.0);
        if line {
            let mut l = p.part(
                format!("legend/{i}/key"),
                Rect {
                    x,
                    y: cy,
                    width: kw,
                    height: 0.0,
                },
                Draw::Shape(ShapeKind {
                    preset: "line".into(),
                    adjust: Vec::new(),
                }),
            );
            l.stroke = color;
            l.stroke_width = LINE_WIDTH;
            p.items.push(l);
        } else {
            p.fill(
                format!("legend/{i}/key"),
                Rect {
                    x,
                    y: cy - key / 2.0,
                    width: key,
                    height: key,
                },
                color,
            );
        }
        p.label(
            text,
            format!("legend/{i}"),
            name,
            &style,
            (x + kw + size * 0.4, cy),
            (0.0, 0.5),
        )?;
    }
    Ok(rect)
}

fn point_on(cx: f32, cy: f32, radius: f32, deg: f32) -> (f32, f32) {
    let a = (deg - 90.0).to_radians();
    (cx + radius * a.cos(), cy + radius * a.sin())
}

/// Slices clockwise from 12 o'clock, as Office draws them.
fn pie(p: &mut Painter<'_>, text: &mut TextEngine, chart: &Chart, plot: Rect) -> Result<()> {
    let s = &chart.series[0];
    let total: f64 = s.values.iter().sum();
    let d = plot.width;
    let (c, outer) = (d / 2.0, d / 2.0);
    let inner = if chart.kind == ChartType::Doughnut {
        outer * HOLE
    } else {
        0.0
    };
    let mut start = 0.0f32;
    for (j, v) in s.values.iter().enumerate() {
        let sweep = (*v / total * 360.0) as f32;
        if sweep <= 0.0 {
            continue;
        }
        let end = start + sweep;
        let large = u8::from(sweep > 180.0);
        let d = if sweep >= 359.99 {
            let ring = |r: f32, dir: u8| {
                format!(
                    "M {} {c} A {r} {r} 0 1 {dir} {} {c} A {r} {r} 0 1 {dir} {} {c} Z ",
                    c - r,
                    c + r,
                    c - r
                )
            };
            let mut d = ring(outer, 1);
            if inner > 0.0 {
                d.push_str(&ring(inner, 0));
            }
            d
        } else {
            let (x0, y0) = point_on(c, c, outer, start);
            let (x1, y1) = point_on(c, c, outer, end);
            if inner > 0.0 {
                let (x2, y2) = point_on(c, c, inner, end);
                let (x3, y3) = point_on(c, c, inner, start);
                format!(
                    "M {x0} {y0} A {outer} {outer} 0 {large} 1 {x1} {y1} L {x2} {y2} A {inner} {inner} 0 {large} 0 {x3} {y3} Z"
                )
            } else {
                format!("M {c} {c} L {x0} {y0} A {outer} {outer} 0 {large} 1 {x1} {y1} Z")
            }
        };
        let slice = p.path(format!("slice/{j}"), plot, d);
        slice.fill = s.colors[j];
        slice.stroke = Color(255, 255, 255, 255);
        slice.stroke_width = SEPARATOR;
        slice.line_join = LineJoin::Round;
        if chart.data_labels != DataLabels::None {
            let value = match chart.data_labels {
                DataLabels::Percent => format_value(*v / total, "0%")?,
                DataLabels::Category => chart.categories[j].clone(),
                _ => format_value(*v, &chart.format)?,
            };
            let radius = if inner > 0.0 {
                (outer + inner) / 2.0
            } else {
                outer * 0.62
            };
            let (x, y) = point_on(plot.x + c, plot.y + c, radius, start + sweep / 2.0);
            let mut ls = p.style.clone();
            ls.color = ink(s.colors[j]);
            p.label(
                text,
                format!("value/0/{j}"),
                &value,
                &ls,
                (x, y),
                (0.5, 0.5),
            )?;
        }
        start = end;
    }
    Ok(())
}

/// Smooth curves through points: Catmull-Rom segments as cubic Béziers.
fn polyline(points: &[(f32, f32)], smooth: bool, origin: (f32, f32)) -> String {
    let at = |i: usize| {
        let (x, y) = points[i.min(points.len() - 1)];
        (x - origin.0, y - origin.1)
    };
    let mut d = format!("M {} {}", at(0).0, at(0).1);
    for i in 1..points.len() {
        let (b, c) = (at(i - 1), at(i));
        if smooth {
            let a = at(i.saturating_sub(2));
            let e = at(i + 1);
            d.push_str(&format!(
                " C {} {} {} {} {} {}",
                b.0 + (c.0 - a.0) / 6.0,
                b.1 + (c.1 - a.1) / 6.0,
                c.0 - (e.0 - b.0) / 6.0,
                c.1 - (e.1 - b.1) / 6.0,
                c.0,
                c.1
            ));
        } else {
            d.push_str(&format!(" L {} {}", c.0, c.1));
        }
    }
    d
}

#[allow(clippy::too_many_arguments)]
fn cartesian(
    p: &mut Painter<'_>,
    text: &mut TextEngine,
    chart: &Chart,
    plot: Rect,
    a: &Axis,
    x_axis: Option<&Axis>,
    format: &str,
    grid_color: Color,
    axis_color: Color,
    gap: f32,
    diagnostic: &mut dyn FnMut(&str, String),
) -> Result<()> {
    let style = p.style.clone();
    let horizontal = chart.horizontal;
    let span = |a: &Axis, v: f64| ((v.clamp(a.min, a.max) - a.min) / (a.max - a.min)) as f32;
    let value_at = |v: f64| {
        if horizontal {
            plot.x + span(a, v) * plot.width
        } else {
            plot.bottom() - span(a, v) * plot.height
        }
    };
    let cross = 0.0f64.clamp(a.min, a.max);
    // Gridlines and value-axis tick labels.
    for (i, t) in ticks(a).into_iter().enumerate() {
        let at = value_at(t);
        let value = format_value(t, format)?;
        if horizontal {
            if chart.grid {
                p.line(
                    format!("grid/{i}"),
                    (at, plot.y),
                    (at, plot.bottom()),
                    grid_color,
                );
            }
            p.label(
                text,
                format!("tick/{i}"),
                &value,
                &style,
                (at, plot.bottom() + gap),
                (0.5, 0.0),
            )?;
        } else {
            if chart.grid {
                p.line(
                    format!("grid/{i}"),
                    (plot.x, at),
                    (plot.right(), at),
                    grid_color,
                );
            }
            p.label(
                text,
                format!("tick/{i}"),
                &value,
                &style,
                (plot.x - gap, at),
                (1.0, 0.5),
            )?;
        }
    }
    let n = chart.categories.len();
    // Where point i sits along the category (or x) axis.
    let along: Box<dyn Fn(usize) -> f32> = match x_axis {
        Some(x) => {
            let xs = chart
                .categories
                .iter()
                .map(|c| c.parse::<f64>().unwrap())
                .collect::<Vec<_>>();
            let x = *x;
            Box::new(move |i| plot.x + span(&x, xs[i]) * plot.width)
        }
        None if horizontal => Box::new(move |i| plot.y + (i as f32 + 0.5) * plot.height / n as f32),
        None => Box::new(move |i| plot.x + (i as f32 + 0.5) * plot.width / n as f32),
    };
    let slot = if horizontal { plot.height } else { plot.width } / n as f32;
    match x_axis {
        Some(x) => {
            for (i, t) in ticks(x).into_iter().enumerate() {
                let at = plot.x + span(x, t) * plot.width;
                p.label(
                    text,
                    format!("x-tick/{i}"),
                    &general(t),
                    &style,
                    (at, plot.bottom() + gap),
                    (0.5, 0.0),
                )?;
            }
            let y = value_at(cross);
            p.line("axis".into(), (plot.x, y), (plot.right(), y), axis_color);
        }
        None => {
            for (i, c) in chart.categories.iter().enumerate() {
                let rect = if horizontal {
                    p.label(
                        text,
                        format!("label/{i}"),
                        c,
                        &style,
                        (plot.x - gap, along(i)),
                        (1.0, 0.5),
                    )?
                } else {
                    p.label(
                        text,
                        format!("label/{i}"),
                        c,
                        &style,
                        (along(i), plot.bottom() + gap),
                        (0.5, 0.0),
                    )?
                };
                let size = if horizontal { rect.height } else { rect.width };
                if size > slot + 0.5 {
                    p.reported.push(format!("{}/label/{i}", p.parent.id));
                    diagnostic(
                        "chart_label_overflow",
                        format!(
                            "category label {c:?} needs {size:.0}px; each category has {slot:.0}px (shorten it, widen the chart, or use orient=\"horizontal\")"
                        ),
                    );
                }
            }
            let at = value_at(cross);
            if horizontal {
                p.line("axis".into(), (at, plot.y), (at, plot.bottom()), axis_color);
            } else {
                p.line("axis".into(), (plot.x, at), (plot.right(), at), axis_color);
            }
        }
    }
    let totals = shares(chart);
    let label_of = |s: &Series, i: usize| format_value(s.values[i], &chart.format);
    // Areas first, then bars, then lines, as the native chart stacks its groups.
    let mut base = vec![0.0f64; n];
    for (si, s) in chart
        .series
        .iter()
        .enumerate()
        .filter(|(_, s)| s.kind == ChartType::Area)
    {
        let stack = stacked(chart, s);
        let tops = (0..n)
            .map(|i| plotted(chart, &totals, s, i) + if stack { base[i] } else { 0.0 })
            .collect::<Vec<_>>();
        let mut pts = (0..n)
            .map(|i| (along(i), value_at(tops[i])))
            .collect::<Vec<_>>();
        pts.extend(
            (0..n)
                .rev()
                .map(|i| (along(i), value_at(if stack { base[i] } else { cross }))),
        );
        let mut d = polyline(&pts, false, (plot.x, plot.y));
        d.push_str(" Z");
        p.path(format!("area/{si}"), plot, d).fill = s.colors[0];
        if stack {
            base = tops;
        }
    }
    let bars = chart
        .series
        .iter()
        .enumerate()
        .filter(|(_, s)| s.kind == ChartType::Bar)
        .collect::<Vec<_>>();
    let stack = chart.stack != Stack::None && chart.kind == ChartType::Bar;
    let groups = if stack { 1 } else { bars.len().max(1) } as f32;
    let bw = slot / (groups + GAP);
    for i in 0..n {
        let (mut pos, mut neg) = (0.0f64, 0.0f64);
        let start = along(i) - slot / 2.0 + bw * GAP / 2.0;
        for (j, (si, s)) in bars.iter().enumerate() {
            let v = plotted(chart, &totals, s, i);
            let (from, to) = if !stack {
                (cross, v)
            } else if v >= 0.0 {
                pos += v;
                (pos - v, pos)
            } else {
                neg += v;
                (neg - v, neg)
            };
            let (p0, p1) = (value_at(from), value_at(to));
            let offset = start + if stack { 0.0 } else { j as f32 * bw };
            let rect = if horizontal {
                Rect {
                    x: p0.min(p1),
                    y: offset,
                    width: (p1 - p0).abs(),
                    height: bw,
                }
            } else {
                Rect {
                    x: offset,
                    y: p0.min(p1),
                    width: bw,
                    height: (p1 - p0).abs(),
                }
            };
            p.fill(format!("bar/{si}/{i}"), rect, s.colors[0]);
            if chart.data_labels == DataLabels::Value {
                let value = label_of(s, i)?;
                let mid = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
                if stack {
                    let mut ls = style.clone();
                    ls.color = ink(s.colors[0]);
                    p.label(
                        text,
                        format!("value/{si}/{i}"),
                        &value,
                        &ls,
                        mid,
                        (0.5, 0.5),
                    )?;
                } else {
                    let up = v >= 0.0;
                    let (point, anchor) = match (horizontal, up) {
                        (true, true) => ((rect.right() + gap / 2.0, mid.1), (0.0, 0.5)),
                        (true, false) => ((rect.x - gap / 2.0, mid.1), (1.0, 0.5)),
                        (false, true) => ((mid.0, rect.y - gap / 2.0), (0.5, 1.0)),
                        (false, false) => ((mid.0, rect.bottom() + gap / 2.0), (0.5, 0.0)),
                    };
                    p.label(
                        text,
                        format!("value/{si}/{i}"),
                        &value,
                        &style,
                        point,
                        anchor,
                    )?;
                }
            }
        }
    }
    for (si, s) in chart.series.iter().enumerate() {
        let scatter = s.kind == ChartType::Scatter;
        if s.kind != ChartType::Line && !scatter {
            continue;
        }
        let pts = (0..n)
            .map(|i| (along(i), value_at(s.values[i])))
            .collect::<Vec<_>>();
        if !scatter {
            let line = p.path(
                format!("line/{si}"),
                plot,
                polyline(&pts, chart.smooth, (plot.x, plot.y)),
            );
            line.stroke = s.colors[0];
            line.stroke_width = LINE_WIDTH;
            line.line_join = LineJoin::Round;
            line.line_cap = LineCap::Round;
        }
        for (i, pt) in pts.iter().enumerate() {
            if scatter || chart.markers {
                p.marker(format!("marker/{si}/{i}"), *pt, s.colors[0]);
            }
            if chart.data_labels == DataLabels::Value {
                let above = (pt.0, pt.1 - MARKER / 2.0 - gap / 2.0);
                p.label(
                    text,
                    format!("value/{si}/{i}"),
                    &label_of(s, i)?,
                    &style,
                    above,
                    (0.5, 1.0),
                )?;
            }
        }
    }
    Ok(())
}

// ---- Native Office chart part ----

fn sz(px: f32) -> u32 {
    (px * 75.0).round() as u32
}
fn text_props(style: &TextStyle, size: f32, color: Color, bold: bool) -> String {
    let family = xml(&style.family);
    format!(
        "<c:txPr><a:bodyPr/><a:lstStyle/><a:p><a:pPr><a:defRPr sz=\"{}\" b=\"{}\">{}<a:latin typeface=\"{family}\"/><a:ea typeface=\"{family}\"/></a:defRPr></a:pPr><a:endParaRPr lang=\"zh-CN\"/></a:p></c:txPr>",
        sz(size),
        u8::from(bold),
        fill(color, 1.0)
    )
}
fn line_props(width: f32, color: Color) -> String {
    format!(
        "<c:spPr><a:ln w=\"{}\">{}</a:ln></c:spPr>",
        crate::pptx::emu(width),
        fill(color, 1.0)
    )
}
const NO_LINE: &str = "<c:spPr><a:ln><a:noFill/></a:ln></c:spPr>";
fn manual(r: Rect, frame: Rect, inner: bool, size: bool) -> String {
    let f = |v: f32, o: f32, len: f32| format!("{:.5}", (v - o) / len);
    let mut s = String::from("<c:layout><c:manualLayout>");
    if inner {
        s.push_str("<c:layoutTarget val=\"inner\"/>");
    }
    s.push_str(&format!(
        "<c:xMode val=\"edge\"/><c:yMode val=\"edge\"/><c:x val=\"{}\"/><c:y val=\"{}\"/>",
        f(r.x, frame.x, frame.width),
        f(r.y, frame.y, frame.height)
    ));
    if size {
        s.push_str(&format!(
            "<c:w val=\"{:.5}\"/><c:h val=\"{:.5}\"/>",
            r.width / frame.width,
            r.height / frame.height
        ));
    }
    s.push_str("</c:manualLayout></c:layout>");
    s
}
fn cache_strings(f: &str, values: &[String]) -> String {
    let pts = values
        .iter()
        .enumerate()
        .map(|(i, v)| format!("<c:pt idx=\"{i}\"><c:v>{}</c:v></c:pt>", xml(v)))
        .collect::<String>();
    format!(
        "<c:strRef><c:f>{f}</c:f><c:strCache><c:ptCount val=\"{}\"/>{pts}</c:strCache></c:strRef>",
        values.len()
    )
}
fn cache_numbers(f: &str, values: &[f64]) -> String {
    let pts = values
        .iter()
        .enumerate()
        .map(|(i, v)| format!("<c:pt idx=\"{i}\"><c:v>{v}</c:v></c:pt>"))
        .collect::<String>();
    format!(
        "<c:numRef><c:f>{f}</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val=\"{}\"/>{pts}</c:numCache></c:numRef>",
        values.len()
    )
}

/// Data labels: shown fields, number format, position and text color.
fn data_labels(chart: &Chart, s: &Series, style: &TextStyle) -> String {
    if chart.data_labels == DataLabels::None {
        return String::new();
    }
    let round = chart.kind.is_round();
    let stack = stacked(chart, s) && s.kind == ChartType::Bar;
    let shows = |what: DataLabels| u8::from(chart.data_labels == what);
    let fields = format!(
        "<c:showLegendKey val=\"0\"/><c:showVal val=\"{}\"/><c:showCatName val=\"{}\"/><c:showSerName val=\"0\"/><c:showPercent val=\"{}\"/><c:showBubbleSize val=\"0\"/>",
        shows(DataLabels::Value),
        shows(DataLabels::Category),
        shows(DataLabels::Percent)
    );
    let position = match s.kind {
        ChartType::Doughnut => "",
        ChartType::Bar if !stack => "<c:dLblPos val=\"outEnd\"/>",
        ChartType::Line | ChartType::Scatter => "<c:dLblPos val=\"t\"/>",
        _ => "<c:dLblPos val=\"ctr\"/>",
    };
    let code = if chart.data_labels == DataLabels::Percent {
        "0%"
    } else {
        &chart.format
    };
    let format = format!(
        "<c:numFmt formatCode=\"{}\" sourceLinked=\"0\"/>",
        xml(code)
    );
    let props = |color: Color| text_props(style, style.size, color, false);
    // Labels inside slices and stacked bars take a color that reads on the fill.
    let points = if round {
        s.colors
            .iter()
            .enumerate()
            .map(|(j, c)| {
                format!(
                    "<c:dLbl><c:idx val=\"{j}\"/>{format}<c:spPr><a:noFill/><a:ln><a:noFill/></a:ln></c:spPr>{}{position}{fields}</c:dLbl>",
                    props(ink(*c))
                )
            })
            .collect()
    } else {
        String::new()
    };
    let color = if round || stack {
        ink(s.colors[0])
    } else {
        style.color
    };
    format!(
        "<c:dLbls>{points}{format}<c:spPr><a:noFill/><a:ln><a:noFill/></a:ln></c:spPr>{}{position}{fields}</c:dLbls>",
        props(color)
    )
}

fn series_xml(chart: &Chart, i: usize, s: &Series, style: &TextStyle) -> String {
    let col = column(i + 1);
    let end = chart.categories.len() + 1;
    let tx = cache_strings(&format!("Sheet1!${col}$1"), std::slice::from_ref(&s.name));
    let color = s.colors[0];
    let solid = format!(
        "<c:spPr>{}<a:ln><a:noFill/></a:ln></c:spPr>",
        fill(color, 1.0)
    );
    let marker = |on: bool| {
        if on {
            format!(
                "<c:marker><c:symbol val=\"circle\"/><c:size val=\"{}\"/><c:spPr>{}<a:ln><a:noFill/></a:ln></c:spPr></c:marker>",
                (MARKER * 0.75).round(),
                fill(color, 1.0)
            )
        } else {
            "<c:marker><c:symbol val=\"none\"/></c:marker>".into()
        }
    };
    let (props, extra) = match s.kind {
        ChartType::Bar => (solid, "<c:invertIfNegative val=\"0\"/>".to_owned()),
        ChartType::Area => (solid, String::new()),
        ChartType::Line => (
            format!(
                "<c:spPr><a:ln w=\"{}\" cap=\"rnd\">{}<a:round/></a:ln></c:spPr>",
                crate::pptx::emu(LINE_WIDTH),
                fill(color, 1.0)
            ),
            marker(chart.markers),
        ),
        ChartType::Scatter => (
            "<c:spPr><a:ln><a:noFill/></a:ln></c:spPr>".into(),
            marker(true),
        ),
        ChartType::Pie | ChartType::Doughnut => (
            String::new(),
            s.colors
                .iter()
                .enumerate()
                .map(|(j, c)| {
                    format!(
                        "<c:dPt><c:idx val=\"{j}\"/><c:bubble3D val=\"0\"/><c:spPr>{}<a:ln w=\"{}\"><a:solidFill><a:srgbClr val=\"FFFFFF\"/></a:solidFill></a:ln></c:spPr></c:dPt>",
                        fill(*c, 1.0),
                        crate::pptx::emu(SEPARATOR)
                    )
                })
                .collect(),
        ),
    };
    let labels = data_labels(chart, s, style);
    let values = cache_numbers(&format!("Sheet1!${col}$2:${col}${end}"), &s.values);
    let data = if s.kind == ChartType::Scatter {
        let xs = chart
            .categories
            .iter()
            .map(|c| c.parse::<f64>().unwrap())
            .collect::<Vec<_>>();
        format!(
            "<c:xVal>{}</c:xVal><c:yVal>{values}</c:yVal><c:smooth val=\"0\"/>",
            cache_numbers(&format!("Sheet1!$A$2:$A${end}"), &xs)
        )
    } else {
        format!(
            "<c:cat>{}</c:cat><c:val>{values}</c:val>{}",
            cache_strings(&format!("Sheet1!$A$2:$A${end}"), &chart.categories),
            if s.kind == ChartType::Line {
                format!("<c:smooth val=\"{}\"/>", u8::from(chart.smooth))
            } else {
                String::new()
            }
        )
    };
    format!(
        "<c:ser><c:idx val=\"{i}\"/><c:order val=\"{i}\"/><c:tx>{tx}</c:tx>{props}{extra}{labels}{data}</c:ser>"
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn part(
    item: &Item,
    chart: &Chart,
    axis: Option<&Axis>,
    x_axis: Option<&Axis>,
    plot: Rect,
    legend: Option<Rect>,
    title: Option<Rect>,
) -> String {
    let style = &item.text_style;
    let frame = item.rect;
    let series_of = |kind: ChartType| {
        chart
            .series
            .iter()
            .enumerate()
            .filter(|(_, s)| s.kind == kind)
            .map(|(i, s)| series_xml(chart, i, s, style))
            .collect::<String>()
    };
    let grouping = |kind: ChartType, unstacked: &str| {
        if chart.kind != kind {
            unstacked
        } else {
            match chart.stack {
                Stack::None => unstacked,
                Stack::Normal => "stacked",
                Stack::Percent => "percentStacked",
            }
        }
        .to_owned()
    };
    let axes = "<c:axId val=\"100\"/><c:axId val=\"200\"/>";
    let mut groups = String::new();
    match chart.kind {
        ChartType::Pie => groups.push_str(&format!(
            "<c:pieChart><c:varyColors val=\"1\"/>{}<c:firstSliceAng val=\"0\"/></c:pieChart>",
            series_of(ChartType::Pie)
        )),
        ChartType::Doughnut => groups.push_str(&format!(
            "<c:doughnutChart><c:varyColors val=\"1\"/>{}<c:firstSliceAng val=\"0\"/><c:holeSize val=\"{}\"/></c:doughnutChart>",
            series_of(ChartType::Doughnut),
            (HOLE * 100.0) as u32
        )),
        ChartType::Scatter => groups.push_str(&format!(
            "<c:scatterChart><c:scatterStyle val=\"lineMarker\"/><c:varyColors val=\"0\"/>{}{axes}</c:scatterChart>",
            series_of(ChartType::Scatter)
        )),
        _ => {
            let area = series_of(ChartType::Area);
            if !area.is_empty() {
                groups.push_str(&format!(
                    "<c:areaChart><c:grouping val=\"{}\"/><c:varyColors val=\"0\"/>{area}{axes}</c:areaChart>",
                    grouping(ChartType::Area, "standard")
                ));
            }
            let bar = series_of(ChartType::Bar);
            if !bar.is_empty() {
                let stacked = chart.kind == ChartType::Bar && chart.stack != Stack::None;
                groups.push_str(&format!(
                    "<c:barChart><c:barDir val=\"{}\"/><c:grouping val=\"{}\"/><c:varyColors val=\"0\"/>{bar}<c:gapWidth val=\"{}\"/>{}{axes}</c:barChart>",
                    if chart.horizontal { "bar" } else { "col" },
                    grouping(ChartType::Bar, "clustered"),
                    (GAP * 100.0) as u32,
                    if stacked { "<c:overlap val=\"100\"/>" } else { "" }
                ));
            }
            let line = series_of(ChartType::Line);
            if !line.is_empty() {
                groups.push_str(&format!(
                    "<c:lineChart><c:grouping val=\"standard\"/><c:varyColors val=\"0\"/>{line}<c:marker val=\"1\"/>{axes}</c:lineChart>"
                ));
            }
        }
    }
    let grid_color = faded(style.color, 0.15);
    let axis_color = faded(style.color, 0.35);
    let gridlines = if chart.grid {
        format!(
            "<c:majorGridlines>{}</c:majorGridlines>",
            line_props(1.0, grid_color)
        )
    } else {
        String::new()
    };
    let scaling = |a: &Axis, orientation: &str| {
        format!(
            "<c:scaling><c:orientation val=\"{orientation}\"/><c:max val=\"{}\"/><c:min val=\"{}\"/></c:scaling>",
            a.max, a.min
        )
    };
    let no_ticks = "<c:majorTickMark val=\"none\"/><c:minorTickMark val=\"none\"/>";
    let value_format = if chart.stack == Stack::Percent {
        "0%"
    } else {
        &chart.format
    };
    let axes = match (axis, x_axis) {
        (Some(a), Some(x)) => format!(
            "<c:valAx><c:axId val=\"100\"/>{}<c:delete val=\"0\"/><c:axPos val=\"b\"/><c:numFmt formatCode=\"General\" sourceLinked=\"0\"/>{no_ticks}<c:tickLblPos val=\"low\"/>{}<c:crossAx val=\"200\"/><c:crosses val=\"autoZero\"/><c:crossBetween val=\"midCat\"/><c:majorUnit val=\"{}\"/></c:valAx><c:valAx><c:axId val=\"200\"/>{}<c:delete val=\"0\"/><c:axPos val=\"l\"/>{gridlines}<c:numFmt formatCode=\"{}\" sourceLinked=\"0\"/>{no_ticks}<c:tickLblPos val=\"low\"/>{NO_LINE}<c:crossAx val=\"100\"/><c:crosses val=\"autoZero\"/><c:crossBetween val=\"midCat\"/><c:majorUnit val=\"{}\"/></c:valAx>",
            scaling(x, "minMax"),
            line_props(1.0, axis_color),
            x.step,
            scaling(a, "minMax"),
            xml(value_format),
            a.step
        ),
        (Some(a), None) => {
            let h = chart.horizontal;
            format!(
                "<c:catAx><c:axId val=\"100\"/><c:scaling><c:orientation val=\"{}\"/></c:scaling><c:delete val=\"0\"/><c:axPos val=\"{}\"/><c:numFmt formatCode=\"General\" sourceLinked=\"1\"/>{no_ticks}<c:tickLblPos val=\"low\"/>{}<c:crossAx val=\"200\"/><c:crosses val=\"autoZero\"/><c:auto val=\"1\"/><c:lblAlgn val=\"ctr\"/><c:lblOffset val=\"100\"/><c:tickLblSkip val=\"1\"/><c:noMultiLvlLbl val=\"0\"/></c:catAx><c:valAx><c:axId val=\"200\"/>{}<c:delete val=\"0\"/><c:axPos val=\"{}\"/>{gridlines}<c:numFmt formatCode=\"{}\" sourceLinked=\"0\"/>{no_ticks}<c:tickLblPos val=\"nextTo\"/>{NO_LINE}<c:crossAx val=\"100\"/><c:crosses val=\"{}\"/><c:crossBetween val=\"between\"/><c:majorUnit val=\"{}\"/></c:valAx>",
                if h { "maxMin" } else { "minMax" },
                if h { "l" } else { "b" },
                line_props(1.0, axis_color),
                scaling(a, "minMax"),
                if h { "b" } else { "l" },
                xml(value_format),
                if h { "max" } else { "autoZero" },
                a.step
            )
        }
        _ => String::new(),
    };
    let title_xml = match (&chart.title, title) {
        (Some(t), Some(rect)) => {
            let ts = (style.size * 1.2).round();
            let family = xml(&style.family);
            format!(
                "<c:title><c:tx><c:rich><a:bodyPr/><a:lstStyle/><a:p><a:pPr><a:defRPr sz=\"{0}\" b=\"1\"/></a:pPr><a:r><a:rPr lang=\"zh-CN\" sz=\"{0}\" b=\"1\">{1}<a:latin typeface=\"{family}\"/><a:ea typeface=\"{family}\"/></a:rPr><a:t>{2}</a:t></a:r></a:p></c:rich></c:tx>{3}<c:overlay val=\"0\"/></c:title><c:autoTitleDeleted val=\"0\"/>",
                sz(ts),
                fill(style.color, 1.0),
                xml(t),
                manual(rect, frame, false, false)
            )
        }
        _ => "<c:autoTitleDeleted val=\"1\"/>".into(),
    };
    let legend_xml = match (chart.legend, legend) {
        (Some(side), Some(rect)) => format!(
            "<c:legend><c:legendPos val=\"{}\"/>{}<c:overlay val=\"0\"/></c:legend>",
            match side {
                Side::Top => "t",
                Side::Right => "r",
                Side::Bottom => "b",
                Side::Left => "l",
            },
            // Office draws wider keys than the preview; a roomier box (same center) keeps every entry.
            manual(
                {
                    let grow = (style.size * 2.0)
                        .min(rect.x - frame.x)
                        .min(frame.right() - rect.right())
                        .max(0.0);
                    Rect {
                        x: rect.x - grow,
                        width: rect.width + 2.0 * grow,
                        ..rect
                    }
                },
                frame,
                false,
                true
            )
        ),
        _ => String::new(),
    };
    format!(
        "<c:chartSpace xmlns:c=\"{C}\" xmlns:a=\"{A}\" xmlns:r=\"{R}\"><c:date1904 val=\"0\"/><c:lang val=\"zh-CN\"/><c:roundedCorners val=\"0\"/><c:chart>{title_xml}<c:plotArea>{}{groups}{axes}<c:spPr><a:noFill/><a:ln><a:noFill/></a:ln></c:spPr></c:plotArea>{legend_xml}<c:plotVisOnly val=\"1\"/><c:dispBlanksAs val=\"gap\"/></c:chart><c:spPr>{}{}</c:spPr>{}<c:externalData r:id=\"rData\"><c:autoUpdate val=\"0\"/></c:externalData></c:chartSpace>",
        manual(plot, frame, true, true),
        fill(item.fill, 1.0),
        if item.stroke_width > 0.0 {
            format!(
                "<a:ln w=\"{}\">{}</a:ln>",
                crate::pptx::emu(item.stroke_width),
                fill(item.stroke, 1.0)
            )
        } else {
            "<a:ln><a:noFill/></a:ln>".into()
        },
        text_props(style, style.size, style.color, false)
    )
}

fn column(mut n: usize) -> String {
    n += 1;
    let mut s = String::new();
    while n > 0 {
        n -= 1;
        s.insert(0, (b'A' + (n % 26) as u8) as char);
        n /= 26;
    }
    s
}

/// The embedded workbook Office opens to edit the data.
pub(crate) fn workbook(chart: &Chart) -> Result<Vec<u8>> {
    let mut pkg = Package::new();
    let ns = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
    pkg.relationships(
        "_rels/.rels",
        &[(
            "rWorkbook".into(),
            format!("{R}/officeDocument"),
            "xl/workbook.xml".into(),
        )],
    );
    pkg.text("xl/workbook.xml",format!("<workbook xmlns=\"{ns}\" xmlns:r=\"{R}\"><sheets><sheet name=\"Sheet1\" sheetId=\"1\" r:id=\"rSheet\"/></sheets></workbook>"),"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml");
    pkg.relationships(
        "xl/_rels/workbook.xml.rels",
        &[(
            "rSheet".into(),
            format!("{R}/worksheet"),
            "worksheets/sheet1.xml".into(),
        )],
    );
    let string = |r: usize, c: usize, value: &str| {
        format!(
            "<c r=\"{}{r}\" t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",
            column(c),
            xml(value)
        )
    };
    let number =
        |r: usize, c: usize, value: f64| format!("<c r=\"{}{r}\"><v>{value}</v></c>", column(c));
    let mut rows = String::from("<row r=\"1\">");
    rows.push_str(&string(
        1,
        0,
        if chart.kind == ChartType::Scatter {
            "X"
        } else {
            "Category"
        },
    ));
    for (i, s) in chart.series.iter().enumerate() {
        rows.push_str(&string(1, i + 1, &s.name));
    }
    rows.push_str("</row>");
    for (i, l) in chart.categories.iter().enumerate() {
        let r = i + 2;
        let first = match l.parse::<f64>() {
            Ok(x) if chart.kind == ChartType::Scatter => number(r, 0, x),
            _ => string(r, 0, l),
        };
        rows.push_str(&format!("<row r=\"{r}\">{first}"));
        for (j, s) in chart.series.iter().enumerate() {
            rows.push_str(&number(r, j + 1, s.values[i]));
        }
        rows.push_str("</row>");
    }
    pkg.text(
        "xl/worksheets/sheet1.xml",
        format!("<worksheet xmlns=\"{ns}\"><sheetData>{rows}</sheetData></worksheet>"),
        "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml",
    );
    pkg.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn number_formats() {
        let f = |v, c| format_value(v, c).unwrap();
        assert_eq!(f(1234.5, "General"), "1234.5");
        assert_eq!(f(0.1 + 0.2, "General"), "0.3");
        assert_eq!(f(1234567.891, "#,##0.00"), "1,234,567.89");
        assert_eq!(f(0.256, "0%"), "26%");
        assert_eq!(f(-1200.0, "\"$\"#,##0"), "-$1,200");
        assert_eq!(f(12.0, "0.0\"万\""), "12.0万");
        assert_eq!(f(1.5, "0.##"), "1.5");
        assert_eq!(f(2.0, "0.##"), "2");
        assert!(format_value(1.0, "yyyy-mm-dd").is_err());
        assert!(format_value(1.0, "0,").is_err());
    }
    #[test]
    fn nice_axes() {
        let mut need = |_: &Axis| Ok(30.0);
        let a = axis((0.0, 37.0), (None, None, None), 300.0, &mut need).unwrap();
        assert_eq!((a.min, a.max, a.step), (0.0, 40.0, 5.0));
        let a = axis((0.0, 37.0), (None, None, None), 100.0, &mut need).unwrap();
        assert_eq!((a.min, a.max, a.step), (0.0, 40.0, 20.0));
        let a = axis((-3.0, 8.0), (None, Some(10.0), Some(2.0)), 300.0, &mut need).unwrap();
        assert_eq!((a.min, a.max, a.step), (-4.0, 10.0, 2.0));
    }
}
