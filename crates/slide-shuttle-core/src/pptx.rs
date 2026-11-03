//! Bounded OOXML writer. Text and data charts are real Office objects.
use crate::model::*;
use crate::render::Command;
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
                    "<Relationship Id=\"{}\" Type=\"{}\" Target=\"{}\"{}/>",
                    xml(id),
                    xml(ty),
                    xml(target),
                    if ty.ends_with("/hyperlink") {
                        " TargetMode=\"External\""
                    } else {
                        ""
                    }
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
    let Some(g) = &item.gradient else {
        return fill(item.fill, item.opacity);
    };
    let stops = g
        .stops
        .iter()
        .map(|(at, c)| {
            format!(
                "<a:gs pos=\"{}\">{}</a:gs>",
                (at * 100000.0).round() as u32,
                color(*c, item.opacity)
            )
        })
        .collect::<String>();
    let shade = if g.radial {
        "<a:path path=\"circle\"><a:fillToRect l=\"50000\" t=\"50000\" r=\"50000\" b=\"50000\"/></a:path>".into()
    } else {
        // DrawingML measures from "to right", CSS from "to top".
        format!(
            "<a:lin ang=\"{}\" scaled=\"0\"/>",
            ((g.angle - 90.0).rem_euclid(360.0) * 60000.0).round() as u32
        )
    };
    format!("<a:gradFill rotWithShape=\"1\"><a:gsLst>{stops}</a:gsLst>{shade}</a:gradFill>")
}
fn stroke(item: &Item) -> String {
    if item.stroke_width <= 0.0 {
        return "<a:ln><a:noFill/></a:ln>".into();
    }
    let dash = match item.dash {
        Dash::Solid => "solid",
        Dash::Dashed => "dash",
        Dash::Dotted => "sysDot",
    };
    let ends = ["headEnd", "tailEnd"]
        .iter()
        .zip(item.markers)
        .filter_map(|(tag, marker)| {
            let kind = match marker? {
                Marker::Arrow => "triangle",
                Marker::Stealth => "stealth",
                Marker::Diamond => "diamond",
                Marker::Oval => "oval",
            };
            Some(format!("<a:{tag} type=\"{kind}\" w=\"med\" len=\"med\"/>"))
        })
        .collect::<String>();
    let cap = match item.line_cap {
        LineCap::Butt => "",
        LineCap::Round => " cap=\"rnd\"",
        LineCap::Square => " cap=\"sq\"",
    };
    let join = match item.line_join {
        LineJoin::Miter => "",
        LineJoin::Round => "<a:round/>",
        LineJoin::Bevel => "<a:bevel/>",
    };
    format!(
        "<a:ln w=\"{}\"{cap}>{}<a:prstDash val=\"{dash}\"/>{join}{ends}</a:ln>",
        emu(item.stroke_width),
        fill(item.stroke, item.opacity)
    )
}
/// Outer shadow; DrawingML gives distance and direction instead of x/y offsets.
fn effects(item: &Item) -> String {
    let Some(s) = item.shadow else {
        return String::new();
    };
    let dir = (s.dy.atan2(s.dx).to_degrees().rem_euclid(360.0) * 60000.0).round() as u32;
    format!(
        "<a:effectLst><a:outerShdw blurRad=\"{}\" dist=\"{}\" dir=\"{dir}\" algn=\"ctr\" rotWithShape=\"0\">{}</a:outerShdw></a:effectLst>",
        emu(s.blur),
        emu(s.dx.hypot(s.dy)),
        color(s.color, item.opacity)
    )
}
fn xfrm(r: Rect, t: Turn) -> String {
    let mut attrs = String::new();
    if t.rotate != 0.0 {
        attrs.push_str(&format!(" rot=\"{}\"", (t.rotate * 60000.0).round() as u32));
    }
    if t.flip_x {
        attrs.push_str(" flipH=\"1\"");
    }
    if t.flip_y {
        attrs.push_str(" flipV=\"1\"");
    }
    format!(
        "<a:xfrm{attrs}><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm>",
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
    let mut shared = Shared {
        resources,
        media: Media::new(),
        charts: 0,
    };
    for (index, page) in scene.pages.iter().enumerate() {
        let n = index + 1;
        let mut rels = vec![(
            "rLayout".into(),
            format!("{R}/slideLayout"),
            "../slideLayouts/slideLayout1.xml".into(),
        )];
        let background = match &page.background.kind {
            Draw::Image { src, fit, .. } => {
                let (rel, width, height) = image_rel(src, &mut pkg, &mut rels, &mut shared)?;
                let (_, crop) = fitted(page.background.rect, width, height, *fit);
                format!(
                    "<a:blipFill dpi=\"0\" rotWithShape=\"1\"><a:blip r:embed=\"{rel}\"/>{crop}<a:stretch><a:fillRect/></a:stretch></a:blipFill>"
                )
            }
            _ => item_fill(&page.background),
        };
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
                    body.push_str(&write_item(&bg, &mut id, &mut pkg, &mut rels, &mut shared)?);
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
                        &mut shared,
                    )?);
                }
                body.push_str("</p:grpSp>");
            } else {
                body.push_str(&write_item(
                    item,
                    &mut id,
                    &mut pkg,
                    &mut rels,
                    &mut shared,
                )?);
            }
        }
        let slide = format!(
            "<p:sld xmlns:p=\"{P}\" xmlns:a=\"{A}\" xmlns:r=\"{R}\"><p:cSld name=\"{}\"><p:bg><p:bgPr>{}<a:effectLst/></p:bgPr></p:bg><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{body}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>",
            xml(&page.id),
            background
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

/// Resource key -> (media part name, pixel width, pixel height); one part per image.
type Media = BTreeMap<String, (String, u32, u32)>;
/// Package-wide state shared by every slide: images are stored once, charts numbered globally.
struct Shared<'a> {
    resources: &'a Resources,
    media: Media,
    charts: usize,
}
/// Keep the original PNG/JPEG bytes unless an EXIF rotation would make viewers that
/// honour it disagree with our unrotated pixels. Decoding also validates the image.
fn media_part(bytes: &[u8]) -> Result<(Vec<u8>, &'static str, u32, u32)> {
    use image::{ImageDecoder, ImageFormat, metadata::Orientation};
    let reader = crate::render::image_reader(bytes)?;
    let format = reader.format();
    let mut decoder = reader.into_decoder().map_err(|e| invalid(e.to_string()))?;
    let upright =
        decoder.orientation().map_err(|e| invalid(e.to_string()))? == Orientation::NoTransforms;
    let image = image::DynamicImage::from_decoder(decoder)
        .map_err(|e| invalid(format!("invalid or excessive image: {e}")))?;
    let (width, height) = (image.width(), image.height());
    Ok(match format {
        Some(ImageFormat::Png) if upright => (bytes.to_vec(), "png", width, height),
        Some(ImageFormat::Jpeg) if upright => (bytes.to_vec(), "jpeg", width, height),
        _ => {
            let mut data = Cursor::new(Vec::new());
            image
                .write_to(&mut data, ImageFormat::Png)
                .map_err(|e| invalid(e.to_string()))?;
            (data.into_inner(), "png", width, height)
        }
    })
}
/// A relationship to the image's media part (written once per package), and its pixel size.
fn image_rel(
    src: &str,
    pkg: &mut Package,
    rels: &mut Vec<(String, String, String)>,
    shared: &mut Shared,
) -> Result<(String, f32, f32)> {
    if !shared.media.contains_key(src) {
        let bytes = shared
            .resources
            .images
            .get(src)
            .ok_or_else(|| invalid("missing PPTX image resource"))?;
        let (bytes, extension, width, height) = media_part(bytes)?;
        let name = format!("image{}.{extension}", shared.media.len() + 1);
        pkg.bytes(
            &format!("ppt/media/{name}"),
            bytes,
            &format!("image/{extension}"),
        );
        shared.media.insert(src.into(), (name, width, height));
    }
    let (name, width, height) = &shared.media[src];
    let rel = format!("rImage{}", rels.len());
    rels.push((
        rel.clone(),
        format!("{R}/image"),
        format!("../media/{name}"),
    ));
    Ok((rel, *width as f32, *height as f32))
}
/// The picture frame and source crop for an object-fit and object-position.
fn fitted(rect: Rect, width: f32, height: f32, fit: ImageFit) -> (Rect, String) {
    let ir = crate::render::image_rect(rect, width, height, fit);
    match fit.mode {
        FitMode::Cover => {
            let side = |cut: f32, whole: f32| (cut / whole * 100000.0).round() as i64;
            let (l, r) = (
                side(rect.x - ir.x, ir.width),
                side(ir.right() - rect.right(), ir.width),
            );
            let (t, b) = (
                side(rect.y - ir.y, ir.height),
                side(ir.bottom() - rect.bottom(), ir.height),
            );
            (
                rect,
                format!("<a:srcRect l=\"{l}\" r=\"{r}\" t=\"{t}\" b=\"{b}\"/>"),
            )
        }
        FitMode::Contain => (ir, String::new()),
        FitMode::Fill => (rect, String::new()),
    }
}
/// Preset geometry with adjust values; a rect with border-radius becomes roundRect.
fn geometry(shape: &ShapeKind, frame: Rect, radius: f32) -> String {
    let (name, adjust) = if shape.is("rect") && radius > 0.0 {
        let v = (radius / frame.width.min(frame.height).max(1.0) * 100000.0)
            .round()
            .min(50000.0) as i64;
        (
            "roundRect",
            format!("<a:gd name=\"adj\" fmla=\"val {v}\"/>"),
        )
    } else {
        (
            shape.preset.as_str(),
            crate::preset::adjust_names(&shape.preset)
                .iter()
                .zip(&shape.adjust)
                .map(|(n, v)| format!("<a:gd name=\"{n}\" fmla=\"val {}\"/>", v.round() as i64))
                .collect(),
        )
    };
    format!("<a:prstGeom prst=\"{name}\"><a:avLst>{adjust}</a:avLst></a:prstGeom>")
}
fn write_item(
    item: &Item,
    id: &mut u32,
    pkg: &mut Package,
    rels: &mut Vec<(String, String, String)>,
    shared: &mut Shared,
) -> Result<String> {
    if matches!(item.kind, Draw::Box) && item.fill.3 == 0 && item.stroke_width <= 0.0 {
        return Ok(String::new());
    }
    let ident = *id;
    *id += 1;
    Ok(match &item.kind {
        Draw::Text {
            paragraphs: content,
            export_wrap,
            anchor,
            ..
        } => format!(
            "<p:sp>{}<p:spPr>{}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom>{}{}{}</p:spPr><p:txBody><a:bodyPr wrap=\"{}\" lIns=\"0\" rIns=\"0\" tIns=\"0\" bIns=\"0\" anchor=\"{}\"><a:noAutofit/></a:bodyPr><a:lstStyle/>{}</p:txBody></p:sp>",
            nv(ident, &item.id, true),
            xfrm(item.rect, item.turn),
            item_fill(item),
            stroke(item),
            effects(item),
            if *export_wrap { "square" } else { "none" },
            match anchor {
                VerticalAlign::Top => "t",
                VerticalAlign::Middle => "ctr",
                VerticalAlign::Bottom => "b",
            },
            paragraphs(content, &item.text_style, item.opacity, rels)?
        ),
        Draw::Image { src, fit, clip } => {
            let (rel, width, height) = image_rel(src, pkg, rels, shared)?;
            let (rect, crop) = fitted(item.rect, width, height, *fit);
            format!(
                "<p:pic><p:nvPicPr><p:cNvPr id=\"{ident}\" name=\"d:{}\"/><p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed=\"{rel}\"><a:alphaModFix amt=\"{}\"/></a:blip>{crop}<a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr>{}{}<a:noFill/>{}{}</p:spPr></p:pic>",
                xml(&item.id),
                (item.opacity * 100000.0) as u32,
                xfrm(rect, item.turn),
                geometry(clip, rect, item.radius),
                stroke(item),
                effects(item)
            )
        }
        Draw::Icon(icon) => {
            let color = item.text_style.color;
            let paths = icon
                .paths
                .iter()
                .map(|p| (p.commands.as_slice(), p.fill, p.stroke))
                .collect::<Vec<_>>();
            let fills = icon.paths.iter().any(|p| p.fill);
            let line = if icon.paths.iter().any(|p| p.stroke) {
                format!(
                    "<a:ln w=\"{}\"{}>{}<a:prstDash val=\"solid\"/>{}</a:ln>",
                    emu(icon.stroke_width),
                    if icon.round { " cap=\"rnd\"" } else { "" },
                    fill(color, item.opacity),
                    if icon.round { "<a:round/>" } else { "" }
                )
            } else {
                "<a:ln><a:noFill/></a:ln>".into()
            };
            format!(
                "<p:sp>{}<p:spPr>{}{}{}{line}</p:spPr></p:sp>",
                nv(ident, &item.id, false),
                xfrm(item.rect, item.turn),
                custom_geometry(item.rect, &paths),
                if fills {
                    fill(color, item.opacity)
                } else {
                    "<a:noFill/>".into()
                }
            )
        }
        Draw::Table {
            grid,
            columns,
            rows,
            ..
        } => table(item, ident, (grid, columns, rows), rels)?,
        Draw::Chart {
            chart_type,
            labels,
            series,
            ..
        } => {
            shared.charts += 1;
            let n = shared.charts;
            let rel = format!("rChart{n}");
            pkg.text(
                &format!("ppt/charts/chart{n}.xml"),
                chart_xml(*chart_type, labels, series, &item.text_style),
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
                custom_geometry(
                    item.rect,
                    &[(&crate::render::item_path(item)?, item.fill.3 > 0, true)],
                )
            } else {
                match &item.kind {
                    Draw::Shape(shape) => geometry(shape, item.rect, item.radius),
                    _ => geometry(&ShapeKind::default(), item.rect, item.radius),
                }
            };
            format!(
                "<p:sp>{}<p:spPr>{}{geom}{}{}{}</p:spPr></p:sp>",
                nv(ident, &item.id, false),
                xfrm(item.rect, item.turn),
                item_fill(item),
                stroke(item),
                effects(item)
            )
        }
    })
}

/// DrawingML paragraphs. Links add external relationships to `rels`.
pub(crate) fn paragraphs(
    content: &[Paragraph],
    base: &TextStyle,
    opacity: f32,
    rels: &mut Vec<(String, String, String)>,
) -> Result<String> {
    let mut out = String::new();
    for p in content {
        let mut style = base.clone();
        if let Some(align) = p.style.get("text-align") {
            style.align = keyword(align, "text-align")?;
        }
        let algn = match style.align {
            Align::Left => "l",
            Align::Center => "ctr",
            Align::Right => "r",
            Align::Justify => "just",
        };
        let before = p
            .style
            .get("margin-top")
            .map(|v| length_px(v))
            .transpose()?
            .unwrap_or(0.0);
        let mut runs = String::new();
        let mut first_run = None;
        for span in &p.spans {
            let mut ts = crate::text::text_style(&span.style, &style)?;
            ts.color.3 = (ts.color.3 as f32 * opacity).round() as u8;
            first_run.get_or_insert_with(|| ts.clone());
            let link = span.href.as_ref().map(|href| {
                let id = format!("rLink{}", rels.len());
                rels.push((id.clone(), format!("{R}/hyperlink"), href.clone()));
                id
            });
            for (i, part) in span.text.split('\n').enumerate() {
                if i > 0 {
                    runs.push_str("<a:br/>");
                }
                if !part.is_empty() {
                    runs.push_str(&format!(
                        "<a:r>{}<a:t xml:space=\"preserve\">{}</a:t></a:r>",
                        run_props(&ts, &span.style, link.as_deref())?,
                        xml(part)
                    ));
                }
            }
        }
        let first = first_run.unwrap_or_else(|| style.clone());
        let step = 1.5 * first.size;
        let (list_attrs, bullet) = match p.list {
            Some(list) => (
                format!(
                    " marL=\"{}\" indent=\"{}\" lvl=\"{}\"",
                    emu(f32::from(list.level + 1) * step),
                    -emu(step),
                    list.level
                ),
                if list.ordered {
                    format!(
                        "<a:buFont typeface=\"{0}\"/><a:buAutoNum type=\"arabicPeriod\" startAt=\"{1}\"/>",
                        xml(&first.family),
                        list.number
                    )
                } else {
                    format!(
                        "<a:buFont typeface=\"{}\"/><a:buChar char=\"{}\"/>",
                        xml(&first.family),
                        if list.level % 2 == 0 { "•" } else { "–" }
                    )
                },
            ),
            None => (" marL=\"0\" indent=\"0\"".into(), "<a:buNone/>".into()),
        };
        out.push_str(&format!(
            "<a:p><a:pPr{list_attrs} algn=\"{algn}\"><a:lnSpc><a:spcPts val=\"{}\"/></a:lnSpc><a:spcBef><a:spcPts val=\"{}\"/></a:spcBef><a:spcAft><a:spcPts val=\"0\"/></a:spcAft>{bullet}</a:pPr>{runs}<a:endParaRPr lang=\"zh-CN\" sz=\"{}\"/></a:p>",
            (style.line_height * 75.0).round() as u32,
            (before * 75.0).round() as u32,
            (first.size * 75.0).round() as u32
        ));
    }
    Ok(out)
}
fn run_props(s: &TextStyle, css: &Css, link: Option<&str>) -> Result<String> {
    let mut attrs = format!(
        " lang=\"zh-CN\" sz=\"{}\" b=\"{}\" i=\"{}\"",
        (s.size * 75.0).round() as u32,
        u8::from(s.weight >= 700),
        u8::from(s.italic)
    );
    if s.underline {
        attrs.push_str(" u=\"sng\"");
    }
    if s.strike {
        attrs.push_str(" strike=\"sngStrike\"");
    }
    if s.letter_spacing != 0.0 {
        attrs.push_str(&format!(
            " spc=\"{}\"",
            (s.letter_spacing * 75.0).round() as i32
        ));
    }
    match css.get("vertical-align").map(String::as_str) {
        Some("super") => attrs.push_str(" baseline=\"30000\""),
        Some("sub") => attrs.push_str(" baseline=\"-25000\""),
        _ => {}
    }
    let highlight = css
        .get("background")
        .map(|v| Color::parse(v))
        .transpose()?
        .map(|c| {
            format!(
                "<a:highlight><a:srgbClr val=\"{}\"/></a:highlight>",
                c.hex()
            )
        })
        .unwrap_or_default();
    let family = xml(&s.family);
    Ok(format!(
        "<a:rPr{attrs}>{}{highlight}<a:latin typeface=\"{family}\"/><a:ea typeface=\"{family}\"/><a:cs typeface=\"{family}\"/>{}</a:rPr>",
        fill(s.color, 1.0),
        link.map(|id| format!("<a:hlinkClick r:id=\"{id}\"/>"))
            .unwrap_or_default()
    ))
}
/// A native table: spans as gridSpan/rowSpan with hMerge/vMerge on covered cells.
fn table(
    item: &Item,
    id: u32,
    (cells, columns, rows): (&[CellBox], &[f32], &[f32]),
    rels: &mut Vec<(String, String, String)>,
) -> Result<String> {
    let grid = columns
        .iter()
        .map(|w| format!("<a:gridCol w=\"{}\"/>", emu(*w)))
        .collect::<String>();
    let line = |tag: &str, b: &CellBorder| {
        if b.width <= 0.0 || b.color.3 == 0 {
            return format!("<a:{tag} w=\"0\"><a:noFill/></a:{tag}>");
        }
        let dash = match b.dash {
            Dash::Solid => "solid",
            Dash::Dashed => "dash",
            Dash::Dotted => "sysDot",
        };
        format!(
            "<a:{tag} w=\"{}\">{}<a:prstDash val=\"{dash}\"/></a:{tag}>",
            emu(b.width),
            fill(b.color, 1.0)
        )
    };
    let mut body = String::new();
    for (r, height) in rows.iter().enumerate() {
        body.push_str(&format!("<a:tr h=\"{}\">", emu(*height)));
        for c in 0..columns.len() {
            let owner = cells
                .iter()
                .find(|b| {
                    (b.row..b.row + b.rowspan).contains(&r)
                        && (b.col..b.col + b.colspan).contains(&c)
                })
                .ok_or_else(|| invalid("table grid has an uncovered cell"))?;
            let mut attrs = String::new();
            if r == owner.row && c == owner.col {
                if owner.colspan > 1 {
                    attrs.push_str(&format!(" gridSpan=\"{}\"", owner.colspan));
                }
                if owner.rowspan > 1 {
                    attrs.push_str(&format!(" rowSpan=\"{}\"", owner.rowspan));
                }
            } else {
                if c == owner.col && owner.colspan > 1 {
                    attrs.push_str(&format!(" gridSpan=\"{}\"", owner.colspan));
                }
                if c > owner.col {
                    attrs.push_str(" hMerge=\"1\"");
                }
                if r > owner.row {
                    attrs.push_str(" vMerge=\"1\"");
                }
            }
            let text = if r == owner.row && c == owner.col {
                paragraphs(&owner.paragraphs, &owner.text_style, 1.0, rels)?
            } else {
                String::new()
            };
            let [t, rr, b, l] = owner.padding;
            body.push_str(&format!(
                "<a:tc{attrs}><a:txBody><a:bodyPr/><a:lstStyle/>{}</a:txBody><a:tcPr marL=\"{}\" marR=\"{}\" marT=\"{}\" marB=\"{}\" anchor=\"{}\">{}{}{}{}{}</a:tcPr></a:tc>",
                if text.is_empty() { "<a:p/>".into() } else { text },
                emu(l),
                emu(rr),
                emu(t),
                emu(b),
                match owner.anchor {
                    VerticalAlign::Top => "t",
                    VerticalAlign::Middle => "ctr",
                    VerticalAlign::Bottom => "b",
                },
                line("lnL", &owner.borders[3]),
                line("lnR", &owner.borders[1]),
                line("lnT", &owner.borders[0]),
                line("lnB", &owner.borders[2]),
                fill(owner.fill, 1.0)
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
/// Custom geometry from page-unit sub-paths, each with its own fill and stroke flags.
fn custom_geometry(frame: Rect, paths: &[(&[Command], bool, bool)]) -> String {
    let pt = |x: f32, y: f32| {
        format!(
            "<a:pt x=\"{}\" y=\"{}\"/>",
            ((x - frame.x) * 1000.0).round() as i64,
            ((y - frame.y) * 1000.0).round() as i64
        )
    };
    let mut list = String::new();
    for (commands, fill, stroke) in paths {
        let mut path = String::new();
        for c in commands.iter() {
            path.push_str(&match *c {
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
        list.push_str(&format!(
            "<a:path w=\"{}\" h=\"{}\" fill=\"{}\" stroke=\"{}\">{path}</a:path>",
            (frame.width * 1000.0).round().max(1.0) as i64,
            (frame.height * 1000.0).round().max(1.0) as i64,
            if *fill { "norm" } else { "none" },
            u8::from(*stroke)
        ));
    }
    format!(
        "<a:custGeom><a:avLst/><a:gdLst/><a:ahLst/><a:cxnLst/><a:rect l=\"0\" t=\"0\" r=\"r\" b=\"b\"/><a:pathLst>{list}</a:pathLst></a:custGeom>"
    )
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

fn chart_xml(
    kind: ChartType,
    labels: &[String],
    series: &[Series],
    text_style: &TextStyle,
) -> String {
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
        let shape = if kind == ChartType::Line {
            format!("<a:ln w=\"28575\">{}</a:ln>", fill(color, 1.0))
        } else {
            fill(color, 1.0)
        };
        ser.push_str(&format!("<c:ser><c:idx val=\"{i}\"/><c:order val=\"{i}\"/><c:tx><c:strRef><c:f>Sheet1!${col}$1</c:f><c:strCache><c:ptCount val=\"1\"/><c:pt idx=\"0\"><c:v>{}</c:v></c:pt></c:strCache></c:strRef></c:tx><c:spPr>{shape}</c:spPr><c:cat><c:strRef><c:f>Sheet1!$A$2:$A${end}</c:f><c:strCache><c:ptCount val=\"{}\"/>{cats}</c:strCache></c:strRef></c:cat><c:val><c:numRef><c:f>Sheet1!${col}$2:${col}${end}</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val=\"{}\"/>{nums}</c:numCache></c:numRef></c:val>{}</c:ser>",xml(&s.name),labels.len(),s.values.len(),if kind==ChartType::Line{"<c:smooth val=\"0\"/>"}else{""}));
    }
    let plot = if kind == ChartType::Line {
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
