//! Bounded OOXML writer. Text and data charts are real Office objects.
use crate::model::*;
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
};

pub(crate) const P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
pub(crate) const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
pub(crate) const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const C: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
const REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const PRES: &str = "application/vnd.openxmlformats-officedocument.presentationml.";

pub(crate) struct Package {
    pub entries: BTreeMap<String, Vec<u8>>,
    types: BTreeMap<String, String>,
}
impl Package {
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            types: BTreeMap::new(),
        }
    }
    pub fn text(&mut self, path: &str, body: String, content_type: &str) {
        self.bytes(
            path,
            format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>{body}")
                .into_bytes(),
            content_type,
        );
    }
    pub fn bytes(&mut self, path: &str, bytes: Vec<u8>, content_type: &str) {
        self.entries.insert(path.into(), bytes);
        self.types.insert(path.into(), content_type.into());
    }
    pub fn relationships(&mut self, path: &str, rels: &[(String, String, String)]) {
        let body = rels
            .iter()
            .map(|(id, ty, target)| {
                format!(
                    "<Relationship Id=\"{}\" Type=\"{}\" Target=\"{}\"/>",
                    xml(id),
                    xml(ty),
                    xml(target)
                )
            })
            .collect::<String>();
        self.text(
            path,
            format!("<Relationships xmlns=\"{REL}\">{body}</Relationships>"),
            "application/vnd.openxmlformats-package.relationships+xml",
        );
    }
    pub fn finish(mut self) -> Result<Vec<u8>> {
        let overrides = self
            .types
            .iter()
            .filter(|(p, _)| !p.ends_with(".rels"))
            .map(|(p, t)| {
                format!(
                    "<Override PartName=\"/{}\" ContentType=\"{}\"/>",
                    xml(p),
                    xml(t)
                )
            })
            .collect::<String>();
        let ct = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>{overrides}</Types>"
        );
        self.entries
            .insert("[Content_Types].xml".into(), ct.into_bytes());
        zip_entries(&self.entries)
    }
}
pub(crate) fn zip_entries(entries: &BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());
    for (path, bytes) in entries {
        writer.start_file(path, options)?;
        writer.write_all(bytes)?;
    }
    Ok(writer.finish()?.into_inner())
}
pub(crate) fn emu(v: f32) -> i64 {
    (f64::from(v) * EMU_PER_PX).round() as i64
}
fn color(c: Color, opacity: f32) -> String {
    format!(
        "<a:srgbClr val=\"{}\"><a:alpha val=\"{}\"/></a:srgbClr>",
        c.hex(),
        (c.3 as f32 / 255.0 * opacity * 100000.0).round() as u32
    )
}
fn fill(c: Color, opacity: f32) -> String {
    if c.3 == 0 {
        "<a:noFill/>".into()
    } else {
        format!("<a:solidFill>{}</a:solidFill>", color(c, opacity))
    }
}
fn item_fill(item: &Item) -> String {
    if let Some(g) = item.gradient {
        format!(
            "<a:gradFill rotWithShape=\"1\"><a:gsLst><a:gs pos=\"0\">{}</a:gs><a:gs pos=\"100000\">{}</a:gs></a:gsLst><a:lin ang=\"{}\" scaled=\"1\"/></a:gradFill>",
            color(g.start, item.opacity),
            color(g.end, item.opacity),
            if g.vertical { 5400000 } else { 0 }
        )
    } else {
        fill(item.fill, item.opacity)
    }
}
fn stroke(item: &Item) -> String {
    if item.stroke_width <= 0.0 {
        "<a:ln><a:noFill/></a:ln>".into()
    } else {
        format!(
            "<a:ln w=\"{}\">{}<a:prstDash val=\"solid\"/></a:ln>",
            emu(item.stroke_width),
            fill(item.stroke, item.opacity)
        )
    }
}
fn xfrm(r: Rect) -> String {
    format!(
        "<a:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm>",
        emu(r.x),
        emu(r.y),
        emu(r.width),
        emu(r.height)
    )
}
fn off_ext(r: Rect) -> String {
    format!(
        "<a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/>",
        emu(r.x),
        emu(r.y),
        emu(r.width),
        emu(r.height)
    )
}
fn nv(id: u32, name: &str, tx: bool) -> String {
    format!(
        "<p:nvSpPr><p:cNvPr id=\"{id}\" name=\"d:{}\"/><p:cNvSpPr{} /><p:nvPr/></p:nvSpPr>",
        xml(name),
        if tx { " txBox=\"1\"" } else { "" }
    )
}

pub fn export_pptx(scene: &Scene, resources: &Resources, export_id: &str) -> Result<Vec<u8>> {
    if scene.has_errors() {
        return Err(invalid("cannot export scene with error diagnostics"));
    }
    if export_id.is_empty() || export_id.len() > 200 {
        return Err(invalid("invalid export id"));
    }
    let mut pkg = Package::new();
    let mut pres_rels = vec![(
        "rMaster".into(),
        format!("{R}/slideMaster"),
        "slideMasters/slideMaster1.xml".into(),
    )];
    let mut slides = String::new();
    let (mut image_count, mut chart_count) = (0usize, 0usize);
    for (index, page) in scene.pages.iter().enumerate() {
        let n = index + 1;
        let mut rels = vec![(
            "rLayout".into(),
            format!("{R}/slideLayout"),
            "../slideLayouts/slideLayout1.xml".into(),
        )];
        let mut id = 2u32;
        let mut body = String::new();
        for item in page.items.iter().filter(|i| i.group.is_none()) {
            if matches!(item.kind, Draw::Group) {
                let gid = id;
                id += 1;
                let r = item.rect;
                body.push_str(&format!("<p:grpSp><p:nvGrpSpPr><p:cNvPr id=\"{gid}\" name=\"d:{}\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm>{}<a:chOff x=\"{}\" y=\"{}\"/><a:chExt cx=\"{}\" cy=\"{}\"/></a:xfrm></p:grpSpPr>",xml(&item.id),off_ext(r),emu(r.x),emu(r.y),emu(r.width),emu(r.height)));
                if item.fill.3 > 0 || item.stroke_width > 0.0 {
                    let mut bg = item.clone();
                    bg.id = format!("{}::background", item.id);
                    bg.kind = Draw::Box;
                    body.push_str(&write_item(
                        &bg,
                        &mut id,
                        &mut pkg,
                        &mut rels,
                        resources,
                        &mut image_count,
                        &mut chart_count,
                    )?);
                }
                for child in page
                    .items
                    .iter()
                    .filter(|i| i.group.as_deref() == Some(&item.id))
                {
                    body.push_str(&write_item(
                        child,
                        &mut id,
                        &mut pkg,
                        &mut rels,
                        resources,
                        &mut image_count,
                        &mut chart_count,
                    )?);
                }
                body.push_str("</p:grpSp>");
            } else {
                body.push_str(&write_item(
                    item,
                    &mut id,
                    &mut pkg,
                    &mut rels,
                    resources,
                    &mut image_count,
                    &mut chart_count,
                )?);
            }
        }
        let slide = format!(
            "<p:sld xmlns:p=\"{P}\" xmlns:a=\"{A}\" xmlns:r=\"{R}\"><p:cSld name=\"{}\"><p:bg><p:bgPr>{}<a:effectLst/></p:bgPr></p:bg><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{body}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>",
            xml(&page.id),
            fill(page.background, 1.0)
        );
        pkg.text(
            &format!("ppt/slides/slide{n}.xml"),
            slide,
            &format!("{PRES}slide+xml"),
        );
        pkg.relationships(&format!("ppt/slides/_rels/slide{n}.xml.rels"), &rels);
        pres_rels.push((
            format!("rSlide{n}"),
            format!("{R}/slide"),
            format!("slides/slide{n}.xml"),
        ));
        slides.push_str(&format!("<p:sldId id=\"{}\" r:id=\"rSlide{n}\"/>", 255 + n));
    }
    pkg.text("ppt/presentation.xml",format!("<p:presentation xmlns:p=\"{P}\" xmlns:a=\"{A}\" xmlns:r=\"{R}\"><p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"rMaster\"/></p:sldMasterIdLst><p:sldIdLst>{slides}</p:sldIdLst><p:sldSz cx=\"{}\" cy=\"{}\"/><p:notesSz cx=\"6858000\" cy=\"9144000\"/></p:presentation>",emu(scene.width),emu(scene.height)),&format!("{PRES}presentation.main+xml"));
    pkg.relationships("ppt/_rels/presentation.xml.rels", &pres_rels);
    pkg.relationships("_rels/.rels",&[("rOffice".into(),format!("{R}/officeDocument"),"ppt/presentation.xml".into()),("rCore".into(),"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties".into(),"docProps/core.xml".into()),("rCustom".into(),format!("{R}/custom-properties"),"docProps/custom.xml".into())]);
    pkg.text("docProps/core.xml",format!("<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>{}</dc:title><dc:creator>SlideShuttle</dc:creator></cp:coreProperties>",xml(&scene.title)),"application/vnd.openxmlformats-package.core-properties+xml");
    pkg.text("docProps/custom.xml",format!("<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/custom-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\"><property fmtid=\"{{D5CDD505-2E9C-101B-9397-08002B2CF9AE}}\" pid=\"2\" name=\"slideShuttleExportId\"><vt:lpwstr>{}</vt:lpwstr></property></Properties>",xml(export_id)),"application/vnd.openxmlformats-officedocument.custom-properties+xml");
    let family = scene
        .pages
        .iter()
        .flat_map(|p| &p.items)
        .next()
        .map(|i| i.text_style.family.as_str())
        .unwrap_or("Noto Sans CJK SC");
    skeleton(&mut pkg, family);
    pkg.finish()
}

#[allow(clippy::too_many_arguments)]
fn write_item(
    item: &Item,
    id: &mut u32,
    pkg: &mut Package,
    rels: &mut Vec<(String, String, String)>,
    resources: &Resources,
    image_count: &mut usize,
    chart_count: &mut usize,
) -> Result<String> {
    if matches!(item.kind, Draw::Box) && item.fill.3 == 0 && item.stroke_width <= 0.0 {
        return Ok(String::new());
    }
    let ident = *id;
    *id += 1;
    Ok(match &item.kind {
        Draw::Text {
            spans, export_wrap, ..
        } => format!(
            "<p:sp>{}<p:spPr>{}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom>{}{}</p:spPr><p:txBody><a:bodyPr wrap=\"{}\" lIns=\"0\" rIns=\"0\" tIns=\"0\" bIns=\"0\" anchor=\"t\"><a:noAutofit/></a:bodyPr><a:lstStyle/>{}</p:txBody></p:sp>",
            nv(ident, &item.id, true),
            xfrm(item.rect),
            item_fill(item),
            stroke(item),
            if *export_wrap { "square" } else { "none" },
            paragraphs(spans, &item.text_style, item.opacity)?
        ),
        Draw::Image { src, fit } => {
            let bytes = resources
                .images
                .get(src)
                .ok_or_else(|| invalid("missing PPTX image resource"))?;
            let decoded = crate::render::decode_image(bytes)?;
            *image_count += 1;
            let name = format!("image{}.png", image_count);
            let rel = format!("rImage{image_count}");
            let mut data = Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(decoded.clone())
                .write_to(&mut data, image::ImageFormat::Png)
                .map_err(|e| invalid(e.to_string()))?;
            pkg.bytes(&format!("ppt/media/{name}"), data.into_inner(), "image/png");
            rels.push((
                rel.clone(),
                format!("{R}/image"),
                format!("../media/{name}"),
            ));
            let (mut rect, mut crop) = (item.rect, String::new());
            if fit == "contain" {
                rect = crate::render::image_rect(
                    rect,
                    decoded.width() as f32,
                    decoded.height() as f32,
                    fit,
                );
            }
            if fit == "cover" {
                let ir = crate::render::image_rect(
                    rect,
                    decoded.width() as f32,
                    decoded.height() as f32,
                    fit,
                );
                let l = ((rect.x - ir.x) / ir.width * 100000.0).round() as i64;
                let t = ((rect.y - ir.y) / ir.height * 100000.0).round() as i64;
                crop = format!("<a:srcRect l=\"{l}\" r=\"{l}\" t=\"{t}\" b=\"{t}\"/>");
            }
            format!(
                "<p:pic><p:nvPicPr><p:cNvPr id=\"{ident}\" name=\"d:{}\"/><p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed=\"{rel}\"><a:alphaModFix amt=\"{}\"/></a:blip>{crop}<a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr>{}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic>",
                xml(&item.id),
                (item.opacity * 100000.0) as u32,
                xfrm(rect)
            )
        }
        Draw::Table { rows, columns, .. } => table(item, ident, rows, columns)?,
        Draw::Chart {
            chart_type,
            labels,
            series,
            ..
        } => {
            *chart_count += 1;
            let n = *chart_count;
            let rel = format!("rChart{n}");
            pkg.text(
                &format!("ppt/charts/chart{n}.xml"),
                chart_xml(chart_type, labels, series, &item.text_style),
                "application/vnd.openxmlformats-officedocument.drawingml.chart+xml",
            );
            pkg.bytes(
                &format!("ppt/embeddings/chart{n}.xlsx"),
                workbook(labels, series)?,
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            );
            pkg.relationships(
                &format!("ppt/charts/_rels/chart{n}.xml.rels"),
                &[(
                    "rData".into(),
                    format!("{R}/package"),
                    format!("../embeddings/chart{n}.xlsx"),
                )],
            );
            rels.push((
                rel.clone(),
                format!("{R}/chart"),
                format!("../charts/chart{n}.xml"),
            ));
            format!(
                "<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id=\"{ident}\" name=\"d:{}\"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm>{}</p:xfrm><a:graphic><a:graphicData uri=\"{C}\"><c:chart xmlns:c=\"{C}\" r:id=\"{rel}\"/></a:graphicData></a:graphic></p:graphicFrame>",
                xml(&item.id),
                off_ext(item.rect)
            )
        }
        Draw::Group => return Err(invalid("unexpected nested group")),
        _ => {
            let geom = if let Draw::Path { .. } = item.kind {
                custom_geometry(item)?
            } else {
                let name = match &item.kind {
                    Draw::Shape(s) if s == "ellipse" => "ellipse",
                    Draw::Shape(s) if s == "line" => "line",
                    _ if item.radius > 0.0 => "roundRect",
                    _ => "rect",
                };
                let adjust = if name == "roundRect" {
                    let v = (item.radius / item.rect.width.min(item.rect.height).max(1.0)
                        * 100000.0)
                        .round()
                        .min(50000.0) as i64;
                    format!("<a:gd name=\"adj\" fmla=\"val {v}\"/>")
                } else {
                    String::new()
                };
                format!("<a:prstGeom prst=\"{name}\"><a:avLst>{adjust}</a:avLst></a:prstGeom>")
            };
            format!(
                "<p:sp>{}<p:spPr>{}{geom}{}{}</p:spPr></p:sp>",
                nv(ident, &item.id, false),
                xfrm(item.rect),
                item_fill(item),
                stroke(item)
            )
        }
    })
}

pub(crate) fn paragraphs(spans: &[Span], base: &TextStyle, opacity: f32) -> Result<String> {
    let mut paragraphs = vec![Vec::<(String, TextStyle)>::new()];
    for span in spans {
        let mut ts = crate::text::text_style(&span.style, base)?;
        ts.color.3 = (ts.color.3 as f32 * opacity).round() as u8;
        for (i, part) in span.text.split('\n').enumerate() {
            if i > 0 {
                paragraphs.push(Vec::new());
            }
            if !part.is_empty() {
                paragraphs
                    .last_mut()
                    .unwrap()
                    .push((part.into(), ts.clone()));
            }
        }
    }
    let align = match base.align.as_str() {
        "center" => "ctr",
        "right" => "r",
        "justify" => "just",
        _ => "l",
    };
    Ok(paragraphs.iter().map(|runs|{
        let body=runs.iter().map(|(text,s)|format!("<a:r>{}<a:t xml:space=\"preserve\">{}</a:t></a:r>",run_props(s),xml(text))).collect::<String>();
        format!("<a:p><a:pPr algn=\"{align}\"><a:lnSpc><a:spcPts val=\"{}\"/></a:lnSpc><a:spcBef><a:spcPts val=\"0\"/></a:spcBef><a:spcAft><a:spcPts val=\"0\"/></a:spcAft></a:pPr>{body}<a:endParaRPr lang=\"zh-CN\" sz=\"{}\"/></a:p>",(base.line_height*75.0).round()as u32,(base.size*75.0).round()as u32)
    }).collect())
}
fn run_props(s: &TextStyle) -> String {
    format!(
        "<a:rPr lang=\"zh-CN\" sz=\"{}\" b=\"{}\" i=\"{}\">{}<a:latin typeface=\"{}\"/><a:ea typeface=\"{}\"/><a:cs typeface=\"{}\"/></a:rPr>",
        (s.size * 75.0).round() as u32,
        u8::from(s.weight >= 700),
        u8::from(s.italic),
        fill(s.color, 1.0),
        xml(&s.family),
        xml(&s.family),
        xml(&s.family)
    )
}
fn table(item: &Item, id: u32, rows: &[Vec<String>], columns: &[f32]) -> Result<String> {
    let sum: f32 = columns.iter().sum();
    let grid = columns
        .iter()
        .map(|c| format!("<a:gridCol w=\"{}\"/>", emu(item.rect.width * c / sum)))
        .collect::<String>();
    let mut body = String::new();
    for (ri, row) in rows.iter().enumerate() {
        body.push_str(&format!(
            "<a:tr h=\"{}\">",
            emu(item.rect.height / rows.len() as f32)
        ));
        for value in row {
            body.push_str(&format!("<a:tc><a:txBody><a:bodyPr/><a:lstStyle/>{}</a:txBody><a:tcPr marL=\"76200\" marR=\"76200\" marT=\"57150\" marB=\"57150\">",paragraphs(&[Span{text:value.clone(),style:Css::new()}],&item.text_style,1.0)?));
            for side in ["lnL", "lnR", "lnT", "lnB"] {
                body.push_str(&format!("<a:{side} w=\"9525\"><a:solidFill><a:srgbClr val=\"D2D9E3\"/></a:solidFill></a:{side}>"));
            }
            body.push_str(&format!(
                "{}</a:tcPr></a:tc>",
                fill(
                    if ri == 0 {
                        Color(235, 240, 248, 255)
                    } else {
                        Color::WHITE
                    },
                    1.0
                )
            ));
        }
        body.push_str("</a:tr>");
    }
    Ok(format!(
        "<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id=\"{id}\" name=\"d:{}\"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm>{}</p:xfrm><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/table\"><a:tbl><a:tblPr/><a:tblGrid>{grid}</a:tblGrid>{body}</a:tbl></a:graphicData></a:graphic></p:graphicFrame>",
        xml(&item.id),
        off_ext(item.rect)
    ))
}
fn custom_geometry(item: &Item) -> Result<String> {
    use crate::render::Command;
    let mut path = String::new();
    let pt = |x: f32, y: f32| {
        format!(
            "<a:pt x=\"{}\" y=\"{}\"/>",
            ((x - item.rect.x) * 1000.0).round() as i64,
            ((y - item.rect.y) * 1000.0).round() as i64
        )
    };
    for c in crate::render::item_path(item)? {
        path.push_str(&match c {
            Command::M(x, y) => format!("<a:moveTo>{}</a:moveTo>", pt(x, y)),
            Command::L(x, y) => format!("<a:lnTo>{}</a:lnTo>", pt(x, y)),
            Command::C(a, b, c, d, e, f) => format!(
                "<a:cubicBezTo>{}{}{}</a:cubicBezTo>",
                pt(a, b),
                pt(c, d),
                pt(e, f)
            ),
            Command::Z => "<a:close/>".into(),
        });
    }
    Ok(format!(
        "<a:custGeom><a:avLst/><a:gdLst/><a:ahLst/><a:cxnLst/><a:rect l=\"0\" t=\"0\" r=\"r\" b=\"b\"/><a:pathLst><a:path w=\"{}\" h=\"{}\" fill=\"{}\">{path}</a:path></a:pathLst></a:custGeom>",
        (item.rect.width * 1000.0).round().max(1.0) as i64,
        (item.rect.height * 1000.0).round().max(1.0) as i64,
        if item.fill.3 > 0 { "norm" } else { "none" }
    ))
}

fn skeleton(pkg: &mut Package, family: &str) {
    let family = xml(family);
    let tree = "<p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/></p:spTree>";
    pkg.text("ppt/slideMasters/slideMaster1.xml",format!("<p:sldMaster xmlns:p=\"{P}\" xmlns:a=\"{A}\" xmlns:r=\"{R}\"><p:cSld>{tree}</p:cSld><p:clrMap accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" bg1=\"lt1\" bg2=\"lt2\" folHlink=\"folHlink\" hlink=\"hlink\" tx1=\"dk1\" tx2=\"dk2\"/><p:sldLayoutIdLst><p:sldLayoutId id=\"2147483649\" r:id=\"rLayout\"/></p:sldLayoutIdLst><p:txStyles><p:titleStyle/><p:bodyStyle/><p:otherStyle/></p:txStyles></p:sldMaster>"),&format!("{PRES}slideMaster+xml"));
    pkg.relationships(
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        &[
            (
                "rLayout".into(),
                format!("{R}/slideLayout"),
                "../slideLayouts/slideLayout1.xml".into(),
            ),
            (
                "rTheme".into(),
                format!("{R}/theme"),
                "../theme/theme1.xml".into(),
            ),
        ],
    );
    pkg.text("ppt/slideLayouts/slideLayout1.xml",format!("<p:sldLayout xmlns:p=\"{P}\" xmlns:a=\"{A}\" type=\"blank\" preserve=\"1\"><p:cSld name=\"Blank\">{tree}</p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"),&format!("{PRES}slideLayout+xml"));
    pkg.relationships(
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        &[(
            "rMaster".into(),
            format!("{R}/slideMaster"),
            "../slideMasters/slideMaster1.xml".into(),
        )],
    );
    let mut colors = String::new();
    for (name, c) in [
        ("dk1", "111827"),
        ("lt1", "FFFFFF"),
        ("dk2", "334155"),
        ("lt2", "F1F5F9"),
        ("accent1", "2563EB"),
        ("accent2", "10B981"),
        ("accent3", "F59E0B"),
        ("accent4", "8B5CF6"),
        ("accent5", "EC4899"),
        ("accent6", "06B6D4"),
        ("hlink", "2563EB"),
        ("folHlink", "7C3AED"),
    ] {
        colors.push_str(&format!("<a:{name}><a:srgbClr val=\"{c}\"/></a:{name}>"));
    }
    let fill = "<a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>";
    let fills = fill.repeat(3);
    let lines = format!("<a:ln w=\"9525\">{fill}<a:prstDash val=\"solid\"/></a:ln>").repeat(3);
    let effects = "<a:effectStyle><a:effectLst/></a:effectStyle>".repeat(3);
    pkg.text("ppt/theme/theme1.xml",format!("<a:theme xmlns:a=\"{A}\" name=\"Deck\"><a:themeElements><a:clrScheme name=\"Deck\">{colors}</a:clrScheme><a:fontScheme name=\"Deck\"><a:majorFont><a:latin typeface=\"{family}\"/><a:ea typeface=\"{family}\"/><a:cs typeface=\"\"/></a:majorFont><a:minorFont><a:latin typeface=\"{family}\"/><a:ea typeface=\"{family}\"/><a:cs typeface=\"\"/></a:minorFont></a:fontScheme><a:fmtScheme name=\"Deck\"><a:fillStyleLst>{fills}</a:fillStyleLst><a:lnStyleLst>{lines}</a:lnStyleLst><a:effectStyleLst>{effects}</a:effectStyleLst><a:bgFillStyleLst>{fills}</a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>"),"application/vnd.openxmlformats-officedocument.theme+xml");
}

fn chart_xml(kind: &str, labels: &[String], series: &[Series], text_style: &TextStyle) -> String {
    let mut ser = String::new();
    for (i, s) in series.iter().enumerate() {
        let col = column(i + 1);
        let end = labels.len() + 1;
        let color = Color::parse(&s.color).unwrap();
        let cats = labels
            .iter()
            .enumerate()
            .map(|(i, l)| format!("<c:pt idx=\"{i}\"><c:v>{}</c:v></c:pt>", xml(l)))
            .collect::<String>();
        let nums = s
            .values
            .iter()
            .enumerate()
            .map(|(i, v)| format!("<c:pt idx=\"{i}\"><c:v>{v}</c:v></c:pt>"))
            .collect::<String>();
        let shape = if kind == "line" {
            format!("<a:ln w=\"28575\">{}</a:ln>", fill(color, 1.0))
        } else {
            fill(color, 1.0)
        };
        ser.push_str(&format!("<c:ser><c:idx val=\"{i}\"/><c:order val=\"{i}\"/><c:tx><c:strRef><c:f>Sheet1!${col}$1</c:f><c:strCache><c:ptCount val=\"1\"/><c:pt idx=\"0\"><c:v>{}</c:v></c:pt></c:strCache></c:strRef></c:tx><c:spPr>{shape}</c:spPr><c:cat><c:strRef><c:f>Sheet1!$A$2:$A${end}</c:f><c:strCache><c:ptCount val=\"{}\"/>{cats}</c:strCache></c:strRef></c:cat><c:val><c:numRef><c:f>Sheet1!${col}$2:${col}${end}</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val=\"{}\"/>{nums}</c:numCache></c:numRef></c:val>{}</c:ser>",xml(&s.name),labels.len(),s.values.len(),if kind=="line"{"<c:smooth val=\"0\"/>"}else{""}));
    }
    let plot = if kind == "line" {
        format!(
            "<c:lineChart><c:grouping val=\"standard\"/>{ser}<c:axId val=\"100\"/><c:axId val=\"200\"/></c:lineChart>"
        )
    } else {
        format!(
            "<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/>{ser}<c:gapWidth val=\"80\"/><c:axId val=\"100\"/><c:axId val=\"200\"/></c:barChart>"
        )
    };
    let family = xml(&text_style.family);
    let text_props = format!(
        "<c:txPr><a:bodyPr/><a:lstStyle/><a:p><a:pPr><a:defRPr sz=\"1050\"><a:latin typeface=\"{family}\"/><a:ea typeface=\"{family}\"/></a:defRPr></a:pPr><a:endParaRPr/></a:p></c:txPr>"
    );
    format!(
        "<c:chartSpace xmlns:c=\"{C}\" xmlns:a=\"{A}\" xmlns:r=\"{R}\"><c:lang val=\"zh-CN\"/><c:chart><c:autoTitleDeleted val=\"1\"/><c:plotArea><c:layout/>{plot}<c:catAx><c:axId val=\"100\"/><c:scaling><c:orientation val=\"minMax\"/></c:scaling><c:delete val=\"0\"/><c:axPos val=\"b\"/><c:tickLblPos val=\"nextTo\"/><c:crossAx val=\"200\"/><c:crosses val=\"autoZero\"/><c:auto val=\"1\"/><c:lblAlgn val=\"ctr\"/><c:lblOffset val=\"100\"/></c:catAx><c:valAx><c:axId val=\"200\"/><c:scaling><c:orientation val=\"minMax\"/></c:scaling><c:delete val=\"0\"/><c:axPos val=\"l\"/><c:majorGridlines/><c:numFmt formatCode=\"General\" sourceLinked=\"1\"/><c:tickLblPos val=\"nextTo\"/><c:crossAx val=\"100\"/><c:crosses val=\"autoZero\"/><c:crossBetween val=\"between\"/></c:valAx></c:plotArea><c:legend><c:legendPos val=\"b\"/><c:layout/></c:legend><c:plotVisOnly val=\"1\"/></c:chart>{text_props}<c:externalData r:id=\"rData\"><c:autoUpdate val=\"0\"/></c:externalData></c:chartSpace>"
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
fn workbook(labels: &[String], series: &[Series]) -> Result<Vec<u8>> {
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
    let cell = |r: usize, c: usize, value: &str| {
        format!(
            "<c r=\"{}{r}\" t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",
            column(c),
            xml(value)
        )
    };
    let mut rows = String::from("<row r=\"1\">");
    rows.push_str(&cell(1, 0, "Category"));
    for (i, s) in series.iter().enumerate() {
        rows.push_str(&cell(1, i + 1, &s.name));
    }
    rows.push_str("</row>");
    for (i, l) in labels.iter().enumerate() {
        let r = i + 2;
        rows.push_str(&format!("<row r=\"{r}\">{}", cell(r, 0, l)));
        for (j, s) in series.iter().enumerate() {
            rows.push_str(&format!(
                "<c r=\"{}{r}\"><v>{}</v></c>",
                column(j + 1),
                s.values[i]
            ));
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
