use slide_shuttle_core::{import::import_pptx, pptx::export_pptx, *};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
};
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";

fn resources() -> Resources {
    Resources {
        fonts: vec![
            FontResource {
                family: "DejaVu Sans".into(),
                bytes: include_bytes!("fonts/DejaVuSans.ttf").to_vec(),
            },
            FontResource {
                family: "DejaVu Sans".into(),
                bytes: include_bytes!("fonts/DejaVuSans-Bold.ttf").to_vec(),
            },
        ],
        images: BTreeMap::new(),
    }
}
fn parts() -> BTreeMap<String, Vec<u8>> {
    let source = r#"<Deck version="1" width="640px" height="360px" font-family="DejaVu Sans"><Slide id="p"/></Deck>"#;
    let scene = layout(&parse(source).unwrap(), &resources()).unwrap();
    let bytes = export_pptx(&scene, &resources(), "styles-fixture").unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut f = zip.by_index(i).unwrap();
            let mut data = Vec::new();
            f.read_to_end(&mut data).unwrap();
            (f.name().to_owned(), data)
        })
        .collect()
}
fn pack(parts: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (path, data) in parts {
        z.start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        z.write_all(data).unwrap();
    }
    z.finish().unwrap().into_inner()
}
fn slide(objects: &str, bg: &str, map: &str) -> String {
    format!(
        r#"<p:sld xmlns:p="{P}" xmlns:a="{A}"><p:cSld>{bg}<p:spTree><p:nvGrpSpPr/><p:grpSpPr/>{objects}</p:spTree></p:cSld>{map}</p:sld>"#
    )
}
fn shape(id: usize, fill: &str, body: &str, style: &str) -> String {
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="Object {id}"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="190500" y="190500"/><a:ext cx="1905000" cy="1143000"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom>{fill}</p:spPr>{style}{body}</p:sp>"#
    )
}
fn body(paragraph: &str, list: &str) -> String {
    format!(
        r#"<p:txBody><a:bodyPr lIns="0" rIns="0" tIns="0" bIns="0"/><a:lstStyle>{list}</a:lstStyle>{paragraph}</p:txBody>"#
    )
}
fn text(value: &str, rpr: &str, ppr: &str) -> String {
    format!(r#"<a:p>{ppr}<a:r><a:rPr>{rpr}</a:rPr><a:t>{value}</a:t></a:r></a:p>"#)
}
fn recolor_theme(parts: &mut BTreeMap<String, Vec<u8>>) {
    let theme = String::from_utf8(parts["ppt/theme/theme1.xml"].clone()).unwrap();
    let start = theme.find("<a:clrScheme").unwrap();
    let end = theme.find("</a:clrScheme>").unwrap() + "</a:clrScheme>".len();
    let mut theme = theme[..start].to_owned()
        + r#"<a:clrScheme name="Test"><a:dk1><a:srgbClr val="101010"/></a:dk1><a:lt1><a:srgbClr val="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="404040"/></a:dk2><a:lt2><a:srgbClr val="F0F0F0"/></a:lt2><a:accent1><a:srgbClr val="204060"/></a:accent1><a:accent2><a:srgbClr val="20A040"/></a:accent2><a:accent3><a:sysClr val="windowText" lastClr="123456"/></a:accent3><a:accent4><a:srgbClr val="804020"/></a:accent4></a:clrScheme>"#
        + &theme[end..];
    theme = theme.replace("Noto Sans CJK SC", "DejaVu Sans");
    parts.insert("ppt/theme/theme1.xml".into(), theme.into_bytes());
}
fn imported(parts: &BTreeMap<String, Vec<u8>>) -> (import::Imported, Deck, Scene) {
    let imp = import_pptx(&pack(parts)).unwrap();
    let deck = parse(&imp.xml).unwrap();
    let scene = layout(&deck, &resources()).unwrap();
    assert!(!scene.has_errors(), "{:?}\n{}", scene.diagnostics, imp.xml);
    (imp, deck, scene)
}
fn standard_map(bg: &str) -> String {
    format!(
        r#"accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" bg1="{bg}" bg2="lt2" tx1="dk1" tx2="dk2" hlink="hlink" folHlink="folHlink""#
    )
}

#[test]
fn theme_fill_stroke_text_alpha_and_default_style_render_as_actual_colors() {
    let mut parts = parts();
    recolor_theme(&mut parts);
    let bg = r#"<p:bg><p:bgPr><a:solidFill><a:schemeClr val="bg1"/></a:solidFill></p:bgPr></p:bg>"#;
    let solid = r#"<a:solidFill><a:schemeClr val="accent1"><a:alpha val="50000"/></a:schemeClr></a:solidFill><a:ln w="19050"><a:solidFill><a:schemeClr val="accent2"><a:alpha val="25000"/></a:schemeClr></a:solidFill></a:ln>"#;
    parts.insert(
        "ppt/slides/slide1.xml".into(),
        slide(&shape(2, solid, "", ""), bg, "").into_bytes(),
    );
    let presentation=String::from_utf8(parts["ppt/presentation.xml"].clone()).unwrap().replace("</p:presentation>",r#"<p:defaultTextStyle><a:lvl1pPr algn="ctr"><a:lnSpc><a:spcPts val="2400"/></a:lnSpc><a:defRPr sz="1800" b="1"><a:solidFill><a:schemeClr val="accent2"/></a:solidFill><a:latin typeface="+mn-lt"/></a:defRPr></a:lvl1pPr></p:defaultTextStyle></p:presentation>"#);
    parts.insert("ppt/presentation.xml".into(), presentation.into_bytes());
    let (imp, _, scene) = imported(&parts);
    assert!(imp.snapshot.losses.is_empty(), "{:?}", imp.snapshot.losses);
    assert_eq!(scene.pages[0].background, Color(255, 255, 255, 255));
    let item = &scene.pages[0].items[0];
    assert_eq!(item.fill, Color(32, 64, 96, 128));
    assert_eq!(item.stroke, Color(32, 160, 64, 64));
    let png = render::render_png(&scene, 0, &resources(), 1.0).unwrap();
    let image = image::load_from_memory(&png).unwrap().to_rgba8();
    let pixel = image.get_pixel(80, 80);
    for (actual, expected) in pixel.0[..3].iter().zip([143, 159, 175]) {
        assert!((*actual as i16 - expected).abs() <= 1, "{pixel:?}");
    }
    let paragraph = text("Inherited defaults", "", "");
    parts.insert(
        "ppt/slides/slide1.xml".into(),
        slide(&shape(2, "", &body(&paragraph, ""), ""), bg, "").into_bytes(),
    );
    let (imp, _, scene) = imported(&parts);
    assert!(imp.snapshot.losses.is_empty(), "{:?}", imp.snapshot.losses);
    let item = &scene.pages[0].items[0];
    assert_eq!(item.text_style.family, "DejaVu Sans");
    assert_eq!(item.text_style.size, 24.0);
    assert_eq!(item.text_style.weight, 700);
    assert_eq!(item.text_style.align, "center");
    assert_eq!(item.text_style.line_height, 32.0);
    let Draw::Text { glyphs, .. } = &item.kind else {
        panic!()
    };
    assert!(glyphs.iter().all(|g| g.color == Color(32, 160, 64, 255)));
    assert!(
        imp.snapshot
            .inventory
            .keys()
            .any(|k| k.ends_with("#defaultTextStyle")),
        "original default styles remain auditable"
    );
}

#[test]
fn ordered_color_transforms_keep_alpha_separate_from_rgb() {
    let mut parts = parts();
    recolor_theme(&mut parts);
    for (value, transforms, expected) in [
        (
            "accent4",
            r#"<a:tint val="50000"/><a:alpha val="60000"/><a:alphaMod val="50000"/><a:alphaOff val="10000"/>"#,
            Color(205, 192, 189, 102),
        ),
        (
            "dk2",
            r#"<a:lumMod val="50000"/><a:lumOff val="25000"/>"#,
            Color(96, 96, 96, 255),
        ),
        (
            "dk2",
            r#"<a:lumOff val="25000"/><a:lumMod val="50000"/>"#,
            Color(64, 64, 64, 255),
        ),
        ("accent3", "", Color(18, 52, 86, 255)),
    ] {
        let fill = format!(
            r#"<a:solidFill><a:schemeClr val="{value}">{transforms}</a:schemeClr></a:solidFill>"#
        );
        parts.insert(
            "ppt/slides/slide1.xml".into(),
            slide(&shape(2, &fill, "", ""), "", "").into_bytes(),
        );
        let (imp, _, scene) = imported(&parts);
        assert!(imp.snapshot.losses.is_empty(), "{:?}", imp.snapshot.losses);
        assert_eq!(scene.pages[0].items[0].fill, expected);
    }
}

#[test]
fn master_layout_slide_color_maps_and_master_reset_have_correct_precedence() {
    let mut parts = parts();
    recolor_theme(&mut parts);
    let master = String::from_utf8(parts["ppt/slideMasters/slideMaster1.xml"].clone())
        .unwrap()
        .replace("bg1=\"lt1\"", "bg1=\"accent1\"");
    parts.insert(
        "ppt/slideMasters/slideMaster1.xml".into(),
        master.into_bytes(),
    );
    let layout = format!(
        r#"<p:sldLayout xmlns:p="{P}" xmlns:a="{A}"><p:cSld><p:spTree/></p:cSld><p:clrMapOvr><a:overrideClrMapping {}/></p:clrMapOvr></p:sldLayout>"#,
        standard_map("accent2")
    );
    parts.insert(
        "ppt/slideLayouts/slideLayout1.xml".into(),
        layout.into_bytes(),
    );
    let object = shape(
        2,
        r#"<a:solidFill><a:schemeClr val="bg1"/></a:solidFill>"#,
        "",
        "",
    );
    for (mapping, expected) in [
        ("".into(), Color(32, 160, 64, 255)),
        (
            format!(
                r#"<p:clrMapOvr><a:overrideClrMapping {}/></p:clrMapOvr>"#,
                standard_map("dk1")
            ),
            Color(16, 16, 16, 255),
        ),
        (
            "<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>".into(),
            Color(32, 64, 96, 255),
        ),
    ] {
        parts.insert(
            "ppt/slides/slide1.xml".into(),
            slide(&object, "", &mapping).into_bytes(),
        );
        let (_, _, scene) = imported(&parts);
        assert_eq!(scene.pages[0].items[0].fill, expected);
    }
}

#[test]
fn theme_style_matrix_placeholder_colors_and_background_references_resolve() {
    let mut parts = parts();
    recolor_theme(&mut parts);
    let style = r#"<p:style><a:lnRef idx="1"><a:schemeClr val="accent2"/></a:lnRef><a:fillRef idx="1"><a:schemeClr val="accent1"><a:alpha val="50000"/></a:schemeClr></a:fillRef></p:style>"#;
    let bg = r#"<p:bg><p:bgRef idx="1001"><a:schemeClr val="accent3"/></p:bgRef></p:bg>"#;
    parts.insert(
        "ppt/slides/slide1.xml".into(),
        slide(&shape(2, "", "", style), bg, "").into_bytes(),
    );
    let (imp, _, scene) = imported(&parts);
    assert!(imp.snapshot.losses.is_empty(), "{:?}", imp.snapshot.losses);
    assert_eq!(scene.pages[0].background, Color(18, 52, 86, 255));
    assert_eq!(scene.pages[0].items[0].fill, Color(32, 64, 96, 128));
    assert_eq!(scene.pages[0].items[0].stroke, Color(32, 160, 64, 255));
    parts.insert(
        "ppt/slides/slide1.xml".into(),
        slide(&shape(2, "<a:noFill/>", "", style), bg, "").into_bytes(),
    );
    let (_, _, scene) = imported(&parts);
    assert_eq!(scene.pages[0].items[0].fill.3, 0);
}

#[test]
fn local_text_properties_override_inherited_defaults_including_false_flags() {
    let mut parts = parts();
    recolor_theme(&mut parts);
    let presentation=String::from_utf8(parts["ppt/presentation.xml"].clone()).unwrap().replace("</p:presentation>",r#"<p:defaultTextStyle><a:defPPr><a:defRPr sz="1200" b="1" i="1"><a:latin typeface="+mn-lt"/><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></a:defRPr></a:defPPr><a:lvl2pPr algn="ctr"><a:defRPr sz="1500"/></a:lvl2pPr></p:defaultTextStyle></p:presentation>"#);
    parts.insert("ppt/presentation.xml".into(), presentation.into_bytes());
    let p = r#"<a:p><a:pPr lvl="1" algn="r"><a:defRPr sz="1800"/></a:pPr><a:r><a:rPr sz="2400" b="0" i="0"><a:solidFill><a:schemeClr val="accent2"><a:alpha val="75000"/></a:schemeClr></a:solidFill></a:rPr><a:t>Override</a:t></a:r></a:p>"#;
    parts.insert(
        "ppt/slides/slide1.xml".into(),
        slide(
            &shape(
                2,
                "",
                &body(p, r#"<a:lvl2pPr><a:defRPr sz="1600"/></a:lvl2pPr>"#),
                "",
            ),
            "",
            "",
        )
        .into_bytes(),
    );
    let (imp, _, scene) = imported(&parts);
    assert!(imp.snapshot.losses.is_empty(), "{:?}", imp.snapshot.losses);
    let item = &scene.pages[0].items[0];
    assert_eq!(item.text_style.align, "right");
    let Draw::Text { glyphs, .. } = &item.kind else {
        panic!()
    };
    assert!(
        glyphs
            .iter()
            .all(|g| g.size == 32.0 && g.weight == 400 && g.color == Color(32, 160, 64, 191))
    );
}

#[test]
fn unknown_colors_and_transforms_still_report_specific_losses() {
    let mut parts = parts();
    recolor_theme(&mut parts);
    for color in [
        r#"<a:schemeClr val="missingAccent"/>"#,
        r#"<a:srgbClr val="FF0000"><a:unsupportedTransform val="50000"/></a:srgbClr>"#,
    ] {
        parts.insert(
            "ppt/slides/slide1.xml".into(),
            slide(
                &shape(2, &format!("<a:solidFill>{color}</a:solidFill>"), "", ""),
                "",
                "",
            )
            .into_bytes(),
        );
        let (imp, _, _) = imported(&parts);
        assert!(
            imp.snapshot
                .losses
                .iter()
                .any(|l| l.reason.contains("cannot be resolved")
                    || l.reason.contains("unsupportedTransform"))
        );
        assert!(
            !imp.snapshot
                .losses
                .iter()
                .any(|l| l.reason == "schemeClr appearance is not reconstructed")
        );
    }
}

#[test]
fn placeholder_text_inherits_master_then_layout_before_local_runs() {
    let mut parts = parts();
    recolor_theme(&mut parts);
    let master_ph = shape(
        2,
        "",
        &body(
            "<a:p/>",
            r#"<a:lvl1pPr><a:defRPr sz="1700" b="1"/></a:lvl1pPr>"#,
        ),
        "",
    )
    .replace(
        "<p:nvPr/>",
        r#"<p:nvPr><p:ph type="body" idx="0"/></p:nvPr>"#,
    );
    let master = format!(
        r#"<p:sldMaster xmlns:p="{P}" xmlns:a="{A}"><p:cSld><p:spTree>{master_ph}</p:spTree></p:cSld><p:clrMap {}/><p:txStyles><p:bodyStyle><a:lvl1pPr><a:defRPr sz="1500"><a:latin typeface="+mn-lt"/></a:defRPr></a:lvl1pPr></p:bodyStyle></p:txStyles></p:sldMaster>"#,
        standard_map("lt1")
    );
    parts.insert(
        "ppt/slideMasters/slideMaster1.xml".into(),
        master.into_bytes(),
    );
    let layout_ph=shape(2,"",&body(r#"<a:p><a:pPr algn="ctr"><a:defRPr sz="2100" b="0"><a:solidFill><a:schemeClr val="accent2"/></a:solidFill></a:defRPr></a:pPr></a:p>"#,""),"").replace("<p:nvPr/>",r#"<p:nvPr><p:ph type="body" idx="7"/></p:nvPr>"#);
    parts.insert("ppt/slideLayouts/slideLayout1.xml".into(),format!(r#"<p:sldLayout xmlns:p="{P}" xmlns:a="{A}"><p:cSld><p:spTree>{layout_ph}</p:spTree></p:cSld></p:sldLayout>"#).into_bytes());
    let local = shape(2, "", &body(&text("Inherited", "", ""), ""), "").replace(
        "<p:nvPr/>",
        r#"<p:nvPr><p:ph type="body" idx="7"/></p:nvPr>"#,
    );
    parts.insert(
        "ppt/slides/slide1.xml".into(),
        slide(&local, "", "").into_bytes(),
    );
    let (imp, _, scene) = imported(&parts);
    assert!(imp.snapshot.losses.is_empty(), "{:?}", imp.snapshot.losses);
    let item = &scene.pages[0].items[0];
    assert_eq!(item.text_style.family, "DejaVu Sans");
    assert_eq!(item.text_style.size, 28.0);
    assert_eq!(item.text_style.weight, 400);
    assert_eq!(item.text_style.align, "center");
    let Draw::Text { glyphs, .. } = &item.kind else {
        panic!()
    };
    assert!(glyphs.iter().all(|g| g.color == Color(32, 160, 64, 255)));
}

#[test]
fn theme_relationship_paths_and_slide_theme_overrides_are_resolved_by_type() {
    let mut parts = parts();
    recolor_theme(&mut parts);
    let theme = String::from_utf8(parts.remove("ppt/theme/theme1.xml").unwrap()).unwrap();
    let scheme = &theme[theme.find("<a:clrScheme").unwrap()
        ..theme.find("</a:clrScheme>").unwrap() + "</a:clrScheme>".len()];
    let override_xml = format!(
        r#"<a:themeOverride xmlns:a="{A}">{}</a:themeOverride>"#,
        scheme.replace("204060", "ABCDEF")
    );
    parts.insert("custom/base-colors.xml".into(), theme.into_bytes());
    parts.insert("custom/slide-colors.xml".into(), override_xml.into_bytes());
    let master_rels =
        String::from_utf8(parts["ppt/slideMasters/_rels/slideMaster1.xml.rels"].clone())
            .unwrap()
            .replace("../theme/theme1.xml", "../../custom/base-colors.xml");
    parts.insert(
        "ppt/slideMasters/_rels/slideMaster1.xml.rels".into(),
        master_rels.into_bytes(),
    );
    let relationships=String::from_utf8(parts["ppt/slides/_rels/slide1.xml.rels"].clone()).unwrap().replace("</Relationships>",r#"<Relationship Id="rOverride" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/themeOverride" Target="../../custom/slide-colors.xml"/></Relationships>"#);
    parts.insert(
        "ppt/slides/_rels/slide1.xml.rels".into(),
        relationships.into_bytes(),
    );
    parts.insert(
        "ppt/slides/slide1.xml".into(),
        slide(
            &shape(
                2,
                r#"<a:solidFill><a:schemeClr val="accent1"/></a:solidFill>"#,
                "",
                "",
            ),
            "",
            "",
        )
        .into_bytes(),
    );
    let (imp, _, scene) = imported(&parts);
    assert!(imp.snapshot.losses.is_empty(), "{:?}", imp.snapshot.losses);
    assert_eq!(scene.pages[0].items[0].fill, Color(171, 205, 239, 255));
}

#[test]
fn inherited_master_background_and_image_opacity_are_rendered() {
    let mut parts = parts();
    recolor_theme(&mut parts);
    let master=String::from_utf8(parts["ppt/slideMasters/slideMaster1.xml"].clone()).unwrap().replace("<p:cSld>",r#"<p:cSld><p:bg><p:bgPr><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></p:bgPr></p:bg>"#);
    parts.insert(
        "ppt/slideMasters/slideMaster1.xml".into(),
        master.into_bytes(),
    );
    parts.insert(
        "ppt/slides/slide1.xml".into(),
        slide("", "", "").into_bytes(),
    );
    let (_, _, scene) = imported(&parts);
    assert_eq!(scene.pages[0].background, Color(32, 64, 96, 255));
    let mut r = resources();
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        4,
        4,
        image::Rgba([255, 0, 0, 255]),
    ))
    .write_to(&mut bytes, image::ImageFormat::Png)
    .unwrap();
    r.images.insert("photo".into(), bytes.into_inner());
    let deck=parse(r#"<Deck version="1" width="640px" height="360px" font-family="DejaVu Sans"><Slide id="p"><Image id="photo" src="photo" style="width:100px;height:100px;opacity:0.35"/></Slide></Deck>"#).unwrap();
    let scene = layout(&deck, &r).unwrap();
    let bytes = export_pptx(&scene, &r, "image-alpha").unwrap();
    let imp = import_pptx(&bytes).unwrap();
    assert!(imp.snapshot.losses.is_empty(), "{:?}", imp.snapshot.losses);
    r.images = imp.assets;
    let imported = layout(&parse(&imp.xml).unwrap(), &r).unwrap();
    assert_eq!(imported.pages[0].items[0].opacity, 0.35);
    let image = render::render_png(&imported, 0, &r, 1.0).unwrap();
    let pixel = image::load_from_memory(&image).unwrap().to_rgba8();
    assert_eq!(pixel.get_pixel(50, 50).0, [255, 166, 166, 255]);
}
