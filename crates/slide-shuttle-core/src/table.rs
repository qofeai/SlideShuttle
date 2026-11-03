//! Table layout: rows sized to their content, spans, and styles inherited table → row → cell.
use crate::model::*;
use crate::text::{TextEngine, text_style};

/// Top, right, bottom, left.
const PADDING: [f32; 4] = [6.0, 8.0, 6.0, 8.0];
/// Grid lines when the Table sets no border.
pub(crate) const GRID: CellBorder = CellBorder {
    width: 1.0,
    color: Color(210, 217, 227, 255),
    dash: Dash::Solid,
};
const SIDES: [&str; 4] = ["top", "right", "bottom", "left"];

/// Where each cell sits: (row, column, cell).
fn placements(rows: &[TableRow], columns: usize) -> Vec<(usize, usize, &TableCell)> {
    let mut covered = vec![vec![false; columns]; rows.len()];
    let mut out = Vec::new();
    for (r, row) in rows.iter().enumerate() {
        let mut col = 0;
        for cell in &row.cells {
            while col < columns && covered[r][col] {
                col += 1;
            }
            for line in covered.iter_mut().skip(r).take(cell.rowspan) {
                for slot in line.iter_mut().skip(col).take(cell.colspan) {
                    *slot = true;
                }
            }
            out.push((r, col, cell));
            col += cell.colspan;
        }
    }
    out
}

struct Look {
    text: TextStyle,
    fill: Color,
    borders: [CellBorder; 4],
    padding: [f32; 4],
    anchor: VerticalAlign,
}
fn look(table: &TextStyle, grid: CellBorder, row: &TableRow, cell: &TableCell) -> Result<Look> {
    let text = text_style(&cell.style, &text_style(&row.style, table)?)?;
    let pick = |k: &str| cell.style.get(k).or_else(|| row.style.get(k));
    let fill = pick("background")
        .map(|v| Color::parse(v).map_err(|_| invalid("table cells take a solid background color")))
        .transpose()?
        .unwrap_or_default();
    let mut borders = [grid; 4];
    for css in [&row.style, &cell.style] {
        for (i, side) in SIDES.iter().enumerate() {
            for key in [String::from("border"), format!("border-{side}")] {
                if let Some(v) = css.get(&format!("{key}-width")) {
                    borders[i].width = length_px(v)?;
                }
                if let Some(v) = css.get(&format!("{key}-color")) {
                    borders[i].color = Color::parse(v)?;
                }
                if let Some(v) = css.get(&format!("{key}-style")) {
                    borders[i].dash = match v.as_str() {
                        "solid" => Dash::Solid,
                        "dashed" => Dash::Dashed,
                        "dotted" => Dash::Dotted,
                        other => return Err(invalid(format!("unsupported border style {other}"))),
                    };
                }
            }
        }
    }
    let mut padding = PADDING;
    for css in [&row.style, &cell.style] {
        for (i, side) in SIDES.iter().enumerate() {
            if let Some(v) = css.get(&format!("padding-{side}")) {
                padding[i] = length_px(v)?.max(0.0);
            }
        }
    }
    let anchor = pick("vertical-align")
        .map(|v| keyword(v, "cell vertical-align"))
        .transpose()?
        .unwrap_or(VerticalAlign::Middle);
    Ok(Look {
        text,
        fill,
        borders,
        padding,
        anchor,
    })
}
fn widths(columns: &[f32], width: f32) -> Vec<f32> {
    let sum: f32 = columns.iter().sum();
    columns.iter().map(|c| width * c / sum).collect()
}
fn explicit(row: &TableRow) -> Result<Option<f32>> {
    row.style.get("height").map(|v| length_px(v)).transpose()
}

/// Row heights for a table width: an explicit Row height, else the tallest content.
pub(crate) fn row_heights(
    rows: &[TableRow],
    columns: &[f32],
    width: f32,
    table: &TextStyle,
    text: &mut TextEngine,
) -> Result<Vec<f32>> {
    let widths = widths(columns, width);
    let mut heights = rows
        .iter()
        .map(|r| explicit(r).map(|h| h.unwrap_or(0.0)))
        .collect::<Result<Vec<_>>>()?;
    let mut spanning = Vec::new();
    for (r, c, cell) in placements(rows, columns.len()) {
        let look = look(table, GRID, &rows[r], cell)?;
        let inner =
            widths[c..c + cell.colspan].iter().sum::<f32>() - look.padding[1] - look.padding[3];
        let shaped = text.shape(&cell.paragraphs, &look.text, Some(inner.max(1.0)), true)?;
        // An empty cell still keeps one line, so rows do not collapse.
        let content = if shaped.lines == 0 {
            look.text.line_height
        } else {
            shaped.height
        };
        let need = content + look.padding[0] + look.padding[2];
        // A Row height is a minimum, as in CSS and PowerPoint: content can grow it.
        if cell.rowspan == 1 {
            heights[r] = heights[r].max(need);
        } else {
            spanning.push((r, cell.rowspan, need));
        }
    }
    // A taller spanning cell grows the last automatic row it covers, else its last row.
    for (r, span, need) in spanning {
        let have: f32 = heights[r..r + span].iter().sum();
        if need > have {
            let last = (r..r + span)
                .rev()
                .find(|&i| matches!(explicit(&rows[i]), Ok(None)))
                .unwrap_or(r + span - 1);
            heights[last] += need - have;
        }
    }
    Ok(heights)
}

/// Place the cells inside the table's box: the native grid plus fills, text and borders to paint.
pub(crate) fn place(
    table: &Item,
    rows: &[TableRow],
    columns: &[f32],
    grid: CellBorder,
    text: &mut TextEngine,
    diagnostic: &mut dyn FnMut(&str, String),
) -> Result<Draw> {
    let r = table.rect;
    let widths = widths(columns, r.width);
    let mut heights = row_heights(rows, columns, r.width, &table.text_style, text)?;
    let total: f32 = heights.iter().sum();
    if r.height > total + 0.5 {
        // Spare height goes to the automatic rows, or to every row when all are fixed.
        let auto = (0..rows.len())
            .filter(|&i| matches!(explicit(&rows[i]), Ok(None)))
            .collect::<Vec<_>>();
        let share = if auto.is_empty() {
            (0..rows.len()).collect()
        } else {
            auto
        };
        let extra = (r.height - total) / share.len() as f32;
        for i in share {
            heights[i] += extra;
        }
    } else if r.height < total - 0.5 {
        diagnostic(
            "table_overflow",
            format!(
                "table content needs {total:.2}px; the table is {:.2}px tall",
                r.height
            ),
        );
    }
    let xs = std::iter::once(0.0)
        .chain(widths.iter().scan(0.0, |a, w| {
            *a += w;
            Some(*a)
        }))
        .collect::<Vec<_>>();
    let ys = std::iter::once(0.0)
        .chain(heights.iter().scan(0.0, |a, h| {
            *a += h;
            Some(*a)
        }))
        .collect::<Vec<_>>();
    let placed = placements(rows, columns.len());
    let mut looks = placed
        .iter()
        .map(|(row, _, cell)| look(&table.text_style, grid, &rows[*row], cell))
        .collect::<Result<Vec<_>>>()?;
    collapse(&placed, &mut looks, rows.len(), columns.len(), grid);
    let mut grid_cells = Vec::new();
    let mut items = Vec::new();
    let part = |id: String, rect: Rect, kind: Draw| Item {
        id,
        rect,
        group: table.group.clone(),
        kind,
        text_style: table.text_style.clone(),
        source: table.source.clone(),
        ..Item::default()
    };
    for ((row, col, cell), look) in placed.into_iter().zip(looks) {
        let rect = Rect {
            x: r.x + xs[col],
            y: r.y + ys[row],
            width: xs[col + cell.colspan] - xs[col],
            height: ys[row + cell.rowspan] - ys[row],
        };
        if look.fill.3 > 0 {
            let mut fill = part(format!("{}/cell/{row}/{col}", table.id), rect, Draw::Box);
            fill.fill = look.fill;
            items.push(fill);
        }
        let [t, rr, b, l] = look.padding;
        let content = Rect {
            x: rect.x + l,
            y: rect.y + t,
            width: (rect.width - l - rr).max(0.0),
            height: (rect.height - t - b).max(0.0),
        };
        let mut shaped = text.shape(
            &cell.paragraphs,
            &look.text,
            Some(content.width.max(1.0)),
            true,
        )?;
        if shaped.height > content.height + 0.5 {
            diagnostic(
                "table_text_overflow",
                format!(
                    "cell {},{} text needs {:.2}px; the row gives {:.2}px",
                    row + 1,
                    col + 1,
                    shaped.height,
                    content.height
                ),
            );
        }
        let free = (content.height - shaped.height).max(0.0);
        let dy = match look.anchor {
            VerticalAlign::Top => 0.0,
            VerticalAlign::Middle => free / 2.0,
            VerticalAlign::Bottom => free,
        };
        shaped.offset(content.x, content.y + dy);
        let mut label = part(
            format!("{}/text/{row}/{col}", table.id),
            content,
            Draw::Text {
                paragraphs: cell.paragraphs.clone(),
                glyphs: shaped.glyphs,
                marks: shaped.marks,
                line_count: shaped.lines,
                export_wrap: true,
                anchor: look.anchor,
            },
        );
        label.text_style = look.text.clone();
        items.push(label);
        let edges = [
            (rect.x, rect.y, rect.width, 0.0),
            (rect.right(), rect.y, 0.0, rect.height),
            (rect.x, rect.bottom(), rect.width, 0.0),
            (rect.x, rect.y, 0.0, rect.height),
        ];
        for (side, ((x, y, w, h), border)) in edges.iter().zip(look.borders).enumerate() {
            if border.width > 0.0 && border.color.3 > 0 {
                let mut line = part(
                    format!("{}/border/{row}/{col}/{}", table.id, SIDES[side]),
                    Rect {
                        x: *x,
                        y: *y,
                        width: *w,
                        height: *h,
                    },
                    Draw::Shape(ShapeKind {
                        preset: "line".into(),
                        adjust: Vec::new(),
                    }),
                );
                line.stroke = border.color;
                line.stroke_width = border.width;
                line.dash = border.dash;
                items.push(line);
            }
        }
        grid_cells.push(CellBox {
            row,
            col,
            rowspan: cell.rowspan,
            colspan: cell.colspan,
            rect,
            fill: look.fill,
            borders: look.borders,
            padding: look.padding,
            anchor: look.anchor,
            paragraphs: cell.paragraphs.clone(),
            text_style: look.text,
        });
    }
    Ok(Draw::Table {
        grid: grid_cells,
        columns: widths,
        rows: heights,
        cells: items,
    })
}

/// Resolve each shared edge once, like CSS border-collapse: an explicit border beats the
/// table's default grid, then the wider one wins, then the top/left cell's. Both cells get it.
fn collapse(
    placed: &[(usize, usize, &TableCell)],
    looks: &mut [Look],
    rows: usize,
    columns: usize,
    grid: CellBorder,
) {
    let mut owner = vec![vec![0; columns]; rows];
    for (i, (r, c, cell)) in placed.iter().enumerate() {
        for line in owner.iter_mut().skip(*r).take(cell.rowspan) {
            for slot in line.iter_mut().skip(*c).take(cell.colspan) {
                *slot = i;
            }
        }
    }
    let wins = |a: CellBorder, b: CellBorder| {
        if (a == grid) != (b == grid) {
            b != grid
        } else {
            b.width > a.width
        }
    };
    // (first cell, its side, second cell, its side) for every touching pair of grid units.
    let mut pairs = Vec::new();
    for r in 0..rows {
        for c in 0..columns {
            if r + 1 < rows && owner[r][c] != owner[r + 1][c] {
                pairs.push((owner[r][c], 2, owner[r + 1][c], 0));
            }
            if c + 1 < columns && owner[r][c] != owner[r][c + 1] {
                pairs.push((owner[r][c], 1, owner[r][c + 1], 3));
            }
        }
    }
    for (a, sa, b, sb) in pairs {
        let (x, y) = (looks[a].borders[sa], looks[b].borders[sb]);
        let winner = if wins(x, y) { y } else { x };
        looks[a].borders[sa] = winner;
        looks[b].borders[sb] = winner;
    }
}
