use slide_shuttle_core::{
    baseline::{self, Acceptance, Baseline},
    import::inspect_pptx,
    pptx::export_pptx,
    *,
};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
};

fn resources() -> Resources {
    Resources {
        fonts: vec![
            FontResource {
                family: Some("DejaVu Sans".into()),
                bytes: include_bytes!("fonts/DejaVuSans.ttf").to_vec(),
            },
            FontResource {
                family: Some("DejaVu Sans".into()),
                bytes: include_bytes!("fonts/DejaVuSans-Bold.ttf").to_vec(),
            },
        ],
        images: BTreeMap::new(),
    }
}
fn doc(body: &str) -> String {
    format!(
        r#"<Deck width="640px" height="360px" font-family="DejaVu Sans"><Slide id="p">{body}</Slide></Deck>"#
    )
}
fn simple() -> String {
    doc(
        r##"<Box id="flow" style="width:600px;padding:20px;display:flex;flex-direction:column;gap:10px"><Text id="title" style="height:50px;font-size:24px;line-height:32px;font-weight:700">Hello native world</Text><Text id="body" style="height:70px;font-size:18px;line-height:28px">One source, multiple outputs.</Text><Shape id="logo" style="width:80px;height:40px;background:#09A88F"/></Box>"##,
    )
}
fn compile(source: &str) -> (Deck, Resources, Scene, Vec<u8>) {
    let d = parse(source).unwrap();
    let r = resources();
    let s = layout(&d, &r).unwrap();
    assert!(!s.has_errors(), "{:?}", s.diagnostics);
    let bytes = export_pptx(&s, &r, "fixture-export").unwrap();
    (d, r, s, bytes)
}
fn zip_read(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..z.len())
        .map(|i| {
            let mut f = z.by_index(i).unwrap();
            let mut b = Vec::new();
            f.read_to_end(&mut b).unwrap();
            (f.name().into(), b)
        })
        .collect()
}
fn zip_write(entries: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (n, b) in entries {
        z.start_file(n, zip::write::SimpleFileOptions::default())
            .unwrap();
        z.write_all(b).unwrap();
    }
    z.finish().unwrap().into_inner()
}
fn base() -> (Baseline, Deck, Resources, import::Snapshot) {
    let (d, r, _, b) = compile(&simple());
    let snap = inspect_pptx(&b).unwrap();
    let base = Baseline::new(d.clone(), snap.clone(), baseline::profile(&r).unwrap()).unwrap();
    (base, d, r, snap)
}

#[test]
fn native_render_and_editable_package_are_repeatable() {
    let (d, r, s, b) = compile(&simple());
    assert_eq!(export_pptx(&s, &r, "fixture-export").unwrap(), b);
    let png = render::render_png(&s, 0, &r, 1.0).unwrap();
    let image = image::load_from_memory(&png).unwrap();
    assert_eq!((image.width(), image.height()), (640, 360));
    assert_eq!(render::render_png(&s, 0, &r, 1.0).unwrap(), png);
    let pdf = render::render_pdf(&s, &r).unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
    assert_eq!(render::render_pdf(&s, &r).unwrap(), pdf);
    let parts = zip_read(&b);
    let slide = std::str::from_utf8(&parts["ppt/slides/slide1.xml"]).unwrap();
    assert!(slide.contains("<a:t xml:space=\"preserve\">Hello native world"));
    assert!(!parts.keys().any(|p| p.contains("/media/")));
    let imported = import::import_pptx(&b).unwrap();
    assert!(
        imported.snapshot.losses.is_empty(),
        "{:?}",
        imported.snapshot.losses
    );
    let imported = parse_files("deck.xml", &imported.files).unwrap();
    assert_eq!(imported.slides.len(), d.slides.len());
    let scene = layout(&imported, &r).unwrap();
    assert!(!scene.has_errors(), "{:?}", scene.diagnostics);
}
#[test]
fn grid_span_flex_and_component_ids() {
    let source = r#"<Deck font-family="DejaVu Sans"><Token name="gap" value="20px"/><Component name="Label"><Text id="text" style="height:50px">${value}</Text></Component><Slide id="p"><Box id="g" style="width:600px;display:grid;grid-template-columns:1fr 2fr;gap:var(--gap)"><Use id="a" component="Label" value="First"/><Use id="b" component="Label" value="Second"/><Text id="wide" style="grid-column:1 / span 2;height:50px">Wide</Text></Box></Slide></Deck>"#;
    let (_, _, s, _) = compile(source);
    let items = &s.pages[0].items;
    let a = items.iter().find(|i| i.id == "a/text").unwrap();
    let b = items.iter().find(|i| i.id == "b/text").unwrap();
    let wide = items.iter().find(|i| i.id == "wide").unwrap();
    assert!((a.rect.width - 193.3333).abs() < 0.01);
    assert!((b.rect.x - 213.3333).abs() < 0.01);
    assert_eq!(wide.rect.width, 600.0);
}
#[test]
fn rejects_unknown_attributes_css_and_inline_layout() {
    for inner in [
        r#"<Text id="t" src="ignored">X</Text>"#,
        r#"<Text id="t" style="filter:blur(2px)">X</Text>"#,
        r#"<Text id="t"><Span style="width:20px">X</Span></Text>"#,
        r#"<Text id="t">A</Text><Text id="t">B</Text>"#,
    ] {
        assert!(parse(&doc(inner)).is_err(), "{inner}");
    }
    assert!(
        parse(r#"<Deck><Style name="unused">unknown:yes</Style><Slide id="p"/></Deck>"#).is_err()
    );
}
#[test]
fn tokens_entities_and_components_are_bounded() {
    assert!(parse(r#"<!DOCTYPE Deck [<!ENTITY x "bad">]><Deck><Slide id="p"/></Deck>"#).is_err());
    assert!(parse(r#"<Deck><Token name="x" value="var(--x)"/><Slide id="p"><Box id="b" style="width:var(--x)"/></Slide></Deck>"#).is_err());
    assert!(parse(r#"<Deck><Component name="C"><Use id="r" component="C"/></Component><Slide id="p"><Use id="x" component="C"/></Slide></Deck>"#).is_err());
    assert!(parse(&doc(r#"<Path id="x" d="m 1 2 l 3 4"/>"#)).is_err());
}
#[test]
fn later_declarations_override_shorthands_like_css() {
    let source = r##"<Deck width="640px" height="360px" font-family="DejaVu Sans"><Style name="pad">padding-top:40px</Style><Slide id="p"><Box id="a" style="padding-top:50px;padding:0;width:100px;height:100px"><Shape id="x" style="width:10px;height:10px;background:#000"/></Box><Box id="b" class="pad" style="padding:0;width:100px;height:100px"><Shape id="y" style="width:10px;height:10px;background-color:#000;background:#FFF"/></Box></Slide></Deck>"##;
    let (_, _, s, _) = compile(source);
    let item = |id: &str| s.pages[0].items.iter().find(|i| i.id == id).unwrap();
    assert_eq!(item("x").rect.y, 0.0);
    assert_eq!(item("y").rect.y, 100.0);
    assert_eq!(item("y").fill, Color::WHITE);
}
#[test]
fn many_token_references_are_not_cycles() {
    let refs = ["var(--p)"; 4].join(" ");
    let source = format!(
        r#"<Deck font-family="DejaVu Sans"><Token name="p" value="1px"/><Slide id="p"><Box id="b" style="padding:{refs};margin:{refs};gap:var(--p);left:var(--p);top:var(--p);width:var(--p);height:var(--p)"/></Slide></Deck>"#
    );
    assert!(parse(&source).is_ok());
    let fan = (0..15)
        .map(|i| {
            format!(
                r#"<Token name="t{}" value="var(--t{i}) var(--t{i})"/>"#,
                i + 1
            )
        })
        .collect::<String>();
    let source = format!(
        r#"<Deck><Token name="t0" value="1px"/>{fan}<Slide id="p"><Box id="b" style="width:var(--t15)"/></Slide></Deck>"#
    );
    assert!(parse(&source).unwrap_err().to_string().contains("64 KiB"));
}
#[test]
fn errors_name_every_failing_element_with_its_position() {
    let error = parse(&doc(
        r#"<Text id="a" style="filter:x">A</Text><Box id="ok"><Shape id="b" shape="star"/></Box>"#,
    ))
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("line 1, column 76: unsupported CSS property filter"),
        "{error}"
    );
    assert!(
        error.contains("line 1, column 127: Shape shape: unknown variant `star`, expected one of `rect`, `ellipse`, `line`"),
        "{error}"
    );
    let d = parse(&doc(
        r#"<Text id="t" style="width:9px;height:9px;font-weight:700;font-style:italic">X</Text>"#,
    ))
    .unwrap();
    let error = layout(&d, &resources()).unwrap_err().to_string();
    assert!(
        error.starts_with("line 1, column 76: font face not supplied"),
        "{error}"
    );
}
fn project(files: &[(&str, &str)]) -> Result<Deck> {
    let files = files.iter().copied().collect::<BTreeMap<_, _>>();
    parse_project("deck.xml", files["deck.xml"], |path| {
        files
            .get(path)
            .map(|s| s.to_string())
            .ok_or_else(|| Error::Invalid(format!("missing {path}")))
    })
}
#[test]
fn projects_split_pages_and_share_libraries() {
    let deck = project(&[
        ("deck.xml", r#"<Deck font-family="DejaVu Sans"><Library src="design/system.xml"/><Slide src="pages/./a.xml"/><Slide id="inline"><Box id="title"/></Slide><Slide src="pages/b.xml"/></Deck>"#),
        ("design/system.xml", r#"<Library><Token name="gap" value="8px"/><Style name="h">height:40px;padding:var(--gap)</Style><Component name="Label"><Text id="text" class="h">${value}</Text></Component></Library>"#),
        ("pages/a.xml", r#"<Slide id="a"><Text id="title" class="h">A</Text><Use id="l" component="Label" value="x"/></Slide>"#),
        ("pages/b.xml", r#"<Slide id="b"><Text id="title" class="h">B</Text></Slide>"#),
    ])
    .unwrap();
    assert_eq!(
        deck.slides
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>(),
        ["a", "inline", "b"]
    );
    let a = &deck.slides[0].nodes;
    assert_eq!((a[0].id.as_str(), a[1].id.as_str()), ("title", "l/text"));
    assert_eq!(a[0].style["padding-top"], "8px");
    assert_eq!(a[1].source.file, "design/system.xml");
    assert_eq!(deck.slides[2].nodes[0].source.file, "pages/b.xml");
    let (_, _, s, _) = compile(
        r#"<Deck width="640px" height="360px" font-family="DejaVu Sans"><Slide id="a"><Text id="t" style="height:40px">A</Text></Slide><Slide id="b"><Text id="t" style="height:40px">B</Text></Slide></Deck>"#,
    );
    assert_eq!(s.pages[1].items[0].id, "t");
}
#[test]
fn project_errors_name_their_file_and_stay_inside_the_project() {
    let error = project(&[
        ("deck.xml", r#"<Deck><Slide src="pages/a.xml"/><Slide src="../secret.xml"/><Slide src="pages/missing.xml"/></Deck>"#),
        ("pages/a.xml", "<Slide id=\"a\">\n  <Text id=\"t\" style=\"filter:x\">A</Text><Text id=\"t\">B</Text></Slide>"),
    ])
    .unwrap_err()
    .to_string();
    for expected in [
        "deck.xml line 1, column 33: ../secret.xml: references must be relative paths inside the project",
        "deck.xml line 1, column 61: missing pages/missing.xml",
        "pages/a.xml line 2, column 3: unsupported CSS property filter",
        "pages/a.xml line 2, column 41: invalid or duplicate id: t",
    ] {
        assert!(error.contains(expected), "{expected}\n{error}");
    }
    let error = project(&[
        (
            "deck.xml",
            r#"<Deck><Library src="lib.xml"/><Slide id="s"/></Deck>"#,
        ),
        ("lib.xml", r#"<Library><Slide id="x"/></Library>"#),
    ])
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("lib.xml line 1, column 10: a <Library> holds only"),
        "{error}"
    );
}
#[test]
fn declared_font_family_must_be_in_its_file() {
    let mut r = resources();
    r.fonts[0].family = Some("Brand".into());
    let error = layout(&parse(&simple()).unwrap(), &r)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("Brand is not in its file; the file provides: DejaVu Sans"),
        "{error}"
    );
}
#[test]
fn chart_labels_that_do_not_fit_are_errors() {
    let d = parse(&doc(
        r##"<Chart id="c" style="width:300px;height:200px"><Labels>An extremely long category label|B</Labels><Series name="S" color="#2563EB">1 2</Series></Chart>"##,
    ))
    .unwrap();
    let s = layout(&d, &resources()).unwrap();
    assert!(
        s.diagnostics
            .iter()
            .any(|d| d.code == "chart_label_overflow" && d.node_id == "c/label/0")
    );
    assert!(!s.diagnostics.iter().any(|d| d.node_id == "c/label/1"));
}
#[test]
fn missing_fonts_and_glyphs_are_errors() {
    let d = parse(&simple()).unwrap();
    assert!(layout(&d, &Resources::default()).is_err());
    let mut r = resources();
    r.fonts.pop();
    assert!(layout(&d, &r).is_err());
    let d = parse(&doc(r#"<Text id="t" style="height:80px">中文</Text>"#)).unwrap();
    assert!(layout(&d, &resources()).is_err());
}
#[test]
fn overflow_prevents_export_and_render() {
    let d = parse(&doc(
        r#"<Text id="t" style="width:40px;height:10px">Too much text in this tiny box</Text>"#,
    ))
    .unwrap();
    let r = resources();
    let s = layout(&d, &r).unwrap();
    assert!(s.diagnostics.iter().any(|d| d.code == "text_overflow"));
    assert!(export_pptx(&s, &r, "x").is_err());
    assert!(render::render_png(&s, 0, &r, 1.0).is_err());
}

#[test]
fn empty_pptx_text_bodies_preserve_lines_shapes_and_paths() {
    let source = doc(
        r##"<Shape id="line" shape="line" style="position:absolute;left:20px;top:20px;width:384px;height:0px;border:2px solid #FF0000"/><Shape id="oval" shape="ellipse" style="position:absolute;left:30px;top:60px;width:150px;height:60px;background:#09A88F"/><Path id="triangle" d="M 0 0 L 100 0 L 50 100 Z" viewBox="100 100" style="position:absolute;left:220px;top:60px;width:100px;height:100px;background:#1D4ED8"/>"##,
    );
    let (_, r, original, bytes) = compile(&source);
    let expected_png = render::render_png(&original, 0, &r, 1.0).unwrap();
    let mut parts = zip_read(&bytes);
    let slide = String::from_utf8(parts["ppt/slides/slide1.xml"].clone()).unwrap();
    let blank_body = "<p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"en-US\"/></a:p><a:p><a:r><a:t> </a:t></a:r></a:p></p:txBody>";
    let placeholder = format!(
        "<p:sp><p:nvSpPr><p:cNvPr id=\"9\" name=\"Empty placeholder\"/><p:cNvSpPr txBox=\"1\"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x=\"0\" y=\"1905000\"/><a:ext cx=\"3657600\" cy=\"0\"/></a:xfrm><a:noFill/></p:spPr>{blank_body}</p:sp>"
    );
    let slide = slide
        .replace("</p:sp>", &format!("{blank_body}</p:sp>"))
        .replace("</p:spTree>", &format!("{placeholder}</p:spTree>"));
    parts.insert("ppt/slides/slide1.xml".into(), slide.into_bytes());
    let imported = import::import_pptx(&zip_write(&parts)).unwrap();
    assert_eq!(imported.snapshot.objects.len(), 4);
    let deck = parse_files("deck.xml", &imported.files).unwrap();
    let nodes = &deck.slides[0].nodes;
    assert!(matches!(
        &nodes[0].kind,
        Kind::Shape {
            shape: ShapeKind::Line
        }
    ));
    assert!(matches!(
        &nodes[1].kind,
        Kind::Shape {
            shape: ShapeKind::Ellipse
        }
    ));
    assert!(matches!(&nodes[2].kind, Kind::Path { .. }));
    assert!(matches!(&nodes[3].kind, Kind::Text{spans} if spans.iter().all(|s|s.text.is_empty())));
    let scene = layout(&deck, &r).unwrap();
    assert!(!scene.has_errors(), "{:?}", scene.diagnostics);
    assert_eq!(
        scene.pages[0].items.len(),
        4,
        "empty placeholder retains its identity"
    );
    assert_eq!(
        render::render_png(&scene, 0, &r, 1.0).unwrap(),
        expected_png,
        "empty txBody must not change the original line/shape/path pixels"
    );
    assert!(render::render_pdf(&scene, &r).is_ok());
    assert!(export_pptx(&scene, &r, "empty-body-rebuild").is_ok());
}

#[test]
fn visible_pptx_text_in_zero_height_objects_still_reports_overflow() {
    let (_, r, _, bytes) = compile(&doc(
        r#"<Shape id="object" style="width:384px;height:0px"/>"#,
    ));
    let mut parts = zip_read(&bytes);
    let slide = String::from_utf8(parts["ppt/slides/slide1.xml"].clone()).unwrap();
    let body = "<p:txBody><a:bodyPr lIns=\"0\" rIns=\"0\" tIns=\"0\" bIns=\"0\"/><a:lstStyle/><a:p><a:r><a:rPr><a:latin typeface=\"DejaVu Sans\"/></a:rPr><a:t>Visible text</a:t></a:r></a:p></p:txBody>";
    parts.insert(
        "ppt/slides/slide1.xml".into(),
        slide
            .replace("</p:sp>", &format!("{body}</p:sp>"))
            .into_bytes(),
    );
    let imported = import::import_pptx(&zip_write(&parts)).unwrap();
    let deck = parse_files("deck.xml", &imported.files).unwrap();
    assert!(matches!(&deck.slides[0].nodes[0].kind, Kind::Text { .. }));
    let scene = layout(&deck, &r).unwrap();
    assert!(scene.diagnostics.iter().any(|d| d.code == "text_overflow"));
    assert!(render::render_png(&scene, 0, &r, 1.0).is_err());
}
#[test]
fn bounds_and_nested_group_are_checked() {
    let d = parse(&doc(
        r#"<Shape id="t" style="position:absolute;left:630px;width:30px;height:30px"/>"#,
    ))
    .unwrap();
    assert!(layout(&d, &resources()).unwrap().has_errors());
    let d = parse(&doc(r#"<Group id="a"><Group id="b"/></Group>"#)).unwrap();
    assert!(layout(&d, &resources()).is_err());
}
#[test]
fn simple_gradient_is_native_and_not_silently_imported() {
    let (_, r, s, b) = compile(&doc(
        r##"<Shape id="g" style="width:200px;height:100px;background:linear-gradient(to right, #FF0000, #0000FF)"/>"##,
    ));
    let im = image::load_from_memory(&render::render_png(&s, 0, &r, 1.0).unwrap())
        .unwrap()
        .to_rgba8();
    assert!(im.get_pixel(10, 50)[0] > 200);
    assert!(im.get_pixel(190, 50)[2] > 200);
    assert!(render::render_pdf(&s, &r).is_ok());
    assert!(
        std::str::from_utf8(&zip_read(&b)["ppt/slides/slide1.xml"])
            .unwrap()
            .contains("<a:gradFill")
    );
    assert!(
        inspect_pptx(&b)
            .unwrap()
            .losses
            .iter()
            .any(|l| l.reason.contains("gradFill"))
    );
}
#[test]
fn tables_charts_workbook_and_groups_are_real_objects() {
    let body = r##"<Group id="group" style="height:50px"><Text id="gt" style="height:40px">Movable group</Text></Group><Box id="grid" style="display:grid;grid-template-columns:1fr 1fr;width:620px;height:250px;gap:20px"><Table id="tbl" style="height:200px;font-size:16px;line-height:24px"><Row><Cell>Item</Cell><Cell>Count</Cell></Row><Row><Cell>A</Cell><Cell>3</Cell></Row></Table><Chart id="chart" type="line" style="height:230px"><Labels>Q1|Q2</Labels><Series name="Revenue" color="#09A88F">3 8</Series></Chart></Box>"##;
    let (source, resources, _, b) = compile(&doc(body));
    let snapshot = inspect_pptx(&b).unwrap();
    let baseline = Baseline::new(
        source.clone(),
        snapshot.clone(),
        baseline::profile(&resources).unwrap(),
    )
    .unwrap();
    assert!(
        baseline::review(&baseline, &source, &resources, Some(&snapshot), &snapshot)
            .unwrap()
            .changes
            .is_empty()
    );
    let parts = zip_read(&b);
    let slide = std::str::from_utf8(&parts["ppt/slides/slide1.xml"]).unwrap();
    for t in ["<p:grpSp>", "<a:tbl>", "<c:chart"] {
        assert!(slide.contains(t));
    }
    let chart = std::str::from_utf8(&parts["ppt/charts/chart1.xml"]).unwrap();
    assert!(chart.contains("<c:lineChart>"));
    assert!(chart.contains("Sheet1!$B$2:$B$3"));
    let wb = zip_read(&parts["ppt/embeddings/chart1.xlsx"]);
    let sheet = std::str::from_utf8(&wb["xl/worksheets/sheet1.xml"]).unwrap();
    assert!(sheet.contains("<c r=\"B3\"><v>8</v></c>"));
    let imp = import::import_pptx(&b).unwrap();
    let d = parse_files("deck.xml", &imp.files).unwrap();
    assert!(!layout(&d, &resources).unwrap().has_errors());
}
#[test]
fn images_are_resources_and_stay_images() {
    let mut r = resources();
    let image = image::RgbaImage::from_pixel(40, 20, image::Rgba([9, 168, 143, 255]));
    let mut b = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut b, image::ImageFormat::Png)
        .unwrap();
    r.images.insert("image".into(), b.into_inner());
    let d=parse(&doc(r#"<Image id="i" src="image" style="width:200px;height:100px;opacity:0.5;object-fit:cover"/>"#)).unwrap();
    let s = layout(&d, &r).unwrap();
    assert!(render::render_pdf(&s, &r).is_ok());
    let png = render::render_png(&s, 0, &r, 1.0).unwrap();
    let pixel = image::load_from_memory(&png).unwrap().to_rgba8()[(50, 50)];
    assert!(pixel[0] > 120 && pixel[1] > 200);
    let ppt = export_pptx(&s, &r, "image").unwrap();
    assert!(zip_read(&ppt).contains_key("ppt/media/image1.png"));
    assert!(
        inspect_pptx(&ppt)
            .unwrap()
            .objects
            .iter()
            .any(|o| o.kind == "pic")
    );
}
#[test]
fn pptx_keeps_original_jpeg_bytes_in_one_media_part() {
    let mut r = resources();
    let mut jpeg = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        40,
        20,
        image::Rgb([9, 168, 143]),
    ))
    .write_to(&mut jpeg, image::ImageFormat::Jpeg)
    .unwrap();
    let jpeg = jpeg.into_inner();
    r.images.insert("photo".into(), jpeg.clone());
    let d = parse(&doc(r#"<Image id="a" src="photo" style="width:80px;height:40px"/><Image id="b" src="photo" style="width:80px;height:40px"/>"#)).unwrap();
    let s = layout(&d, &r).unwrap();
    let parts = zip_read(&export_pptx(&s, &r, "jpeg").unwrap());
    assert_eq!(parts["ppt/media/image1.jpeg"], jpeg);
    assert_eq!(
        parts.keys().filter(|k| k.starts_with("ppt/media/")).count(),
        1
    );
}
#[test]
fn zip_paths_and_xml_entities_are_rejected() {
    let (_, _, _, b) = compile(&simple());
    let mut p = zip_read(&b);
    p.insert("../escape".into(), vec![1]);
    assert!(inspect_pptx(&zip_write(&p)).is_err());
    p.remove("../escape");
    p.insert(
        "bad.xml".into(),
        b"<!DOCTYPE root [<!ENTITY x 'x'>]><root>&x;</root>".to_vec(),
    );
    assert!(inspect_pptx(&zip_write(&p)).is_err());
}
#[test]
fn unknown_animation_is_in_inventory_and_blocks_rebuild() {
    let (base, d, r, p) = base();
    let mut external = p.clone();
    external
        .inventory
        .insert("ppt/slides/slide1.xml#timing".into(), "animation".into());
    external.digest = "edited".into();
    let diff = baseline::review(&base, &d, &r, Some(&external), &p).unwrap();
    assert!(
        diff.changes
            .iter()
            .any(|c| c.status == "package_changed" && c.blocking)
    );
}
#[test]
fn imported_unmodeled_content_blocks_even_without_external_change() {
    let (mut base, d, r, p) = base();
    base.imported = true;
    base.native.losses.push(import::Loss {
        page: "p".into(),
        object: "notes".into(),
        reason: "notes not reconstructed".into(),
    });
    let diff = baseline::review(&base, &d, &r, None, &p).unwrap();
    assert!(!diff.external_checked);
    assert!(
        diff.changes
            .iter()
            .any(|c| c.status == "unmodeled" && c.blocking)
    );
}
#[test]
fn three_way_changes_and_source_derived_geometry() {
    let (base, mut d, r, p) = base();
    assert!(
        baseline::review(&base, &d, &r, Some(&p), &p)
            .unwrap()
            .changes
            .is_empty()
    );
    let mut e = p.clone();
    e.objects[0].text = "external".into();
    e.digest = "edit".into();
    let diff = baseline::review(&base, &d, &r, Some(&e), &p).unwrap();
    assert!(
        diff.changes
            .iter()
            .any(|c| c.status == "external_unresolved")
    );
    let mut planned = p.clone();
    planned.objects[0].text = "source".into();
    assert!(
        baseline::review(&base, &d, &r, Some(&e), &planned)
            .unwrap()
            .changes
            .iter()
            .any(|c| c.status == "conflict")
    );
    planned.objects[0].text = "external".into();
    assert!(
        baseline::review(&base, &d, &r, Some(&e), &planned)
            .unwrap()
            .changes
            .iter()
            .any(|c| c.status == "adopted")
    );
    let mut planned = p.clone();
    let i = planned
        .objects
        .iter()
        .position(|o| o.name == "d:body")
        .unwrap();
    planned.objects[i].geometry[1] += 100;
    d.slides[0].nodes[0].children[0]
        .style
        .insert("height".into(), "100px".into());
    assert!(
        baseline::review(&base, &d, &r, Some(&p), &planned)
            .unwrap()
            .changes
            .iter()
            .any(|c| c.status == "derived_change")
    );
}
#[test]
fn duplicates_renames_and_native_detail_changes_do_not_pass_as_clean() {
    let (base, d, r, p) = base();
    let mut e = p.clone();
    e.objects.push(e.objects[0].clone());
    assert!(
        baseline::review(&base, &d, &r, Some(&e), &p)
            .unwrap()
            .changes
            .iter()
            .any(|c| c.status == "ambiguous")
    );
    e = p.clone();
    e.objects[0].name = "renamed".into();
    assert!(
        !baseline::review(&base, &d, &r, Some(&e), &p)
            .unwrap()
            .unresolved(None)
            .unwrap()
            .is_empty()
    );
    e = p.clone();
    e.objects[0].native_digest = "inline-run-color-edited".into();
    assert!(
        baseline::review(&base, &d, &r, Some(&e), &p)
            .unwrap()
            .changes
            .iter()
            .any(|c| c.property == "nativeDetail" && c.blocking)
    );
}
#[test]
fn external_deletion_and_source_deletion_of_external_edit_block() {
    let (base, d, r, p) = base();
    let mut e = p.clone();
    e.objects.remove(0);
    assert!(
        !baseline::review(&base, &d, &r, Some(&e), &p)
            .unwrap()
            .unresolved(None)
            .unwrap()
            .is_empty()
    );
    let mut e = p.clone();
    e.objects[0].text = "edited".into();
    let mut planned = p.clone();
    planned.objects.remove(0);
    assert!(
        baseline::review(&base, &d, &r, Some(&e), &planned)
            .unwrap()
            .changes
            .iter()
            .any(|c| c.status == "conflict")
    );
}
#[test]
fn decisions_bind_every_input_and_never_override_invalid_layout() {
    let (base, d, r, p) = base();
    let mut e = p.clone();
    e.objects[0].text = "external".into();
    e.digest = "edited".into();
    let review = baseline::review(&base, &d, &r, Some(&e), &p).unwrap();
    let accept = Acceptance {
        diff_id: review.diff_id.clone(),
        items: review
            .changes
            .iter()
            .filter(|c| c.blocking)
            .map(|c| {
                (
                    c.id.clone(),
                    "Rebuild as a separate static deck; external edit intentionally discarded"
                        .into(),
                )
            })
            .collect(),
    };
    assert!(review.unresolved(Some(&accept)).unwrap().is_empty());
    e.digest = "another-file-version".into();
    let next = baseline::review(&base, &d, &r, Some(&e), &p).unwrap();
    assert!(next.unresolved(Some(&accept)).is_err());
}
#[test]
fn font_profile_drift_requires_review() {
    let (base, d, mut r, p) = base();
    r.fonts.pop();
    let diff = baseline::review(&base, &d, &r, Some(&p), &p).unwrap();
    assert!(
        diff.changes
            .iter()
            .any(|c| c.status == "engine_drift" && c.blocking)
    );
}
