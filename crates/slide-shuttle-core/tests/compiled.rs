use slide_shuttle_core::*;
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
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
fn source(text: &str) -> String {
    format!(
        r#"<Deck width="640px" height="360px" font-family="DejaVu Sans"><Slide id="first"><Text id="t" style="width:500px;height:100px">{text}</Text></Slide><Slide id="second"><Text id="s" style="width:500px;height:100px">Second page</Text></Slide></Deck>"#
    )
}

#[test]
fn font_usage_belongs_to_each_compilation_and_includes_shaped_ligatures() {
    let mut compiler = Compiler::new(resources()).unwrap();
    assert!(
        compiler
            .compile(&source("No collection in viewers"))
            .unwrap()
            .font_usage()
            .is_empty()
    );
    compiler.collect_font_usage(true);
    let first = compiler.compile(&source("office é العربية")).unwrap();
    assert!(first.font_usage()[&(0, 0)].unicodes.contains(&0x301));
    assert!(!first.font_usage()[&(0, 0)].glyphs.is_empty());
    let second = compiler
        .compile(&source("<Span style=\"font-weight:700\">Bold</Span>"))
        .unwrap();
    assert!(second.font_usage().contains_key(&(1, 0)));
    assert!(!first.font_usage().contains_key(&(1, 0)));
    assert!(!second.font_usage()[&(0, 0)].unicodes.contains(&0x301));
}

#[test]
fn compiled_document_is_reusable_and_survives_compiler_drop() {
    let mut compiler = Compiler::new(resources()).unwrap();
    let first = compiler.compile(&source("Original document")).unwrap();
    let png = first.render_png(0, 1.0).unwrap();
    let next = compiler.compile(&source("Changed document")).unwrap();
    assert_eq!(compiler.compilations(), 2);
    assert!(std::ptr::eq(first.resources(), next.resources()));
    assert_ne!(png, next.render_png(0, 1.0).unwrap());
    drop(compiler);
    assert_eq!(first.page_count(), 2);
    assert_eq!(first.render_png(0, 1.0).unwrap(), png);
    assert!(first.render_png(1, 1.0).is_ok());
    assert!(first.render_png(2, 1.0).is_err());
    assert!(first.render_pdf().unwrap().starts_with(b"%PDF-"));
    assert!(first.export_pptx().unwrap().starts_with(b"PK"));
    assert_eq!(
        first.inspect()["pages"][0]["nodes"][0]["text"]["lineCount"],
        1
    );
}

#[test]
fn compiled_resource_snapshots_do_not_alias_future_image_changes() {
    let image = |rgb: [u8; 4]| {
        let mut c = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(10, 10, image::Rgba(rgb)))
            .write_to(&mut c, image::ImageFormat::Png)
            .unwrap();
        c.into_inner()
    };
    let mut r = resources();
    r.images.insert("photo".into(), image([255, 0, 0, 255]));
    let xml = r#"<Deck width="100px" height="100px" font-family="DejaVu Sans"><Slide id="p"><Image id="i" src="photo" style="width:100px;height:100px"/></Slide></Deck>"#;
    let old = Compiler::new(r.clone()).unwrap().compile(xml).unwrap();
    r.images.insert("photo".into(), image([0, 0, 255, 255]));
    let new = Compiler::new(r).unwrap().compile(xml).unwrap();
    assert_ne!(
        old.render_png(0, 1.0).unwrap(),
        new.render_png(0, 1.0).unwrap()
    );
}

#[test]
fn text_measurement_reports_wraps_and_rejects_invalid_requests() {
    let mut compiler = Compiler::new(resources()).unwrap();
    let style = TextStyle {
        family: "DejaVu Sans".into(),
        size: 24.0,
        line_height: 34.0,
        ..TextStyle::default()
    };
    let wide = compiler
        .measure_text("A paragraph that needs several lines", &style, None, true)
        .unwrap();
    let narrow = compiler
        .measure_text(
            "A paragraph that needs several lines",
            &style,
            Some(100.0),
            true,
        )
        .unwrap();
    assert!(wide.width > 100.0);
    assert_eq!(wide.line_count, 1);
    assert!(narrow.line_count > 1);
    assert!(narrow.height > wide.height);
    assert_eq!(
        compiler
            .measure_text("Hello", &style, None, false)
            .unwrap()
            .glyph_count,
        5
    );
    for width in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert!(
            compiler
                .measure_text("x", &style, Some(width), true)
                .is_err()
        );
    }
    let invalid = TextStyle {
        size: f32::NAN,
        ..style.clone()
    };
    assert!(compiler.measure_text("x", &invalid, None, true).is_err());
    assert!(
        compiler
            .measure_text(&"x".repeat(65537), &style, None, true)
            .is_err()
    );
}

#[test]
fn intrinsic_labels_and_explicit_nowrap_preserve_pptx_line_policy() {
    let body = r#"<Box id="row" style="display:flex;justify-content:center;width:600px;height:60px"><Text id="label" style="font-size:26px;line-height:34px;font-weight:700">Arrangement</Text></Box><Text id="fixed" style="width:500px;height:50px">Explicit width paragraph</Text><Text id="nowrap" style="width:500px;height:50px;white-space:nowrap">Do not wrap</Text><Text id="soft" style="width:80px;height:170px;font-size:18px;line-height:24px">This paragraph wraps automatically</Text>"#;
    let xml = format!(
        r#"<Deck width="640px" height="360px" font-family="DejaVu Sans"><Slide id="p">{body}</Slide></Deck>"#
    );
    let document = Compiler::new(resources()).unwrap().compile(&xml).unwrap();
    assert!(!document.scene().has_errors());
    let bytes = document.export_pptx().unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut slide = String::new();
    zip.by_name("ppt/slides/slide1.xml")
        .unwrap()
        .read_to_string(&mut slide)
        .unwrap();
    let dom = roxmltree::Document::parse(&slide).unwrap();
    for (name, expected) in [
        ("label", "none"),
        ("fixed", "square"),
        ("nowrap", "none"),
        ("soft", "square"),
    ] {
        let shape = dom
            .descendants()
            .find(|n| {
                n.tag_name().name() == "sp"
                    && n.descendants()
                        .any(|x| x.attribute("name") == Some(format!("d:{name}").as_str()))
            })
            .unwrap();
        let props = shape
            .descendants()
            .find(|n| n.tag_name().name() == "bodyPr")
            .unwrap();
        assert_eq!(props.attribute("wrap"), Some(expected), "{name}");
    }
}

#[test]
fn invalid_compiled_scenes_can_be_inspected_but_never_exported() {
    let xml = source("This text is much too tall").replace("height:100px", "height:5px");
    let doc = Compiler::new(resources()).unwrap().compile(&xml).unwrap();
    assert_eq!(doc.inspect()["valid"], false);
    assert!(doc.render_png(0, 1.0).is_err());
    assert!(doc.render_pdf().is_err());
    assert!(doc.export_pptx().is_err());
}

#[test]
fn encoded_page_cache_is_bounded_and_validates_misses() {
    let doc = Compiler::new(resources())
        .unwrap()
        .compile(&source("Cached pages"))
        .unwrap();
    let first = doc.render_png(0, 0.5).unwrap();
    assert_eq!(doc.render_png(0, 0.5).unwrap(), first);
    assert_eq!(doc.render_stats().unwrap().png_renders, 1);
    assert_eq!(doc.render_stats().unwrap().png_cache_hits, 1);
    for step in 1..=8 {
        doc.render_png(0, 0.5 + step as f32 * 0.1).unwrap();
    }
    let stats = doc.render_stats().unwrap();
    assert_eq!(stats.cached_pages, 8);
    assert!(stats.cached_bytes <= 32 * 1024 * 1024);
    doc.render_png(0, 0.5).unwrap();
    assert_eq!(doc.render_stats().unwrap().png_renders, 10);
    assert!(doc.render_png(100, 1.0).is_err());
    assert!(doc.render_png(0, f32::NAN).is_err());
}

#[test]
fn asset_aliases_bind_to_explicit_byte_resources() {
    let mut r = resources();
    r.images.insert(
        "palette.png".into(),
        include_bytes!("../../../examples/palette.png").to_vec(),
    );
    let xml = r#"<Deck width="300px" height="180px" font-family="DejaVu Sans"><Asset id="hero" src="palette.png"/><Slide id="p"><Image id="picture" src="hero" style="width:300px;height:180px"/></Slide></Deck>"#;
    let doc = Compiler::new(r).unwrap().compile(xml).unwrap();
    assert!(doc.render_png(0, 1.0).is_ok());
    assert!(doc.export_pptx().is_ok());
    assert!(
        matches!(&doc.deck().slides[0].nodes[0].kind,Kind::Image{src, ..}if src=="palette.png")
    );
}

#[test]
fn font_fallback_is_explicit_reported_and_preserves_source_inheritance() {
    let xml = r#"<Deck width="640px" height="360px" font-family="Missing Family"><Slide id="p"><Box id="box" style="font-weight:700"><Text id="t" style="width:600px;height:100px">Fallback <Span style="font-family:DejaVu Sans;font-weight:400">Exact face</Span></Text></Box></Slide></Deck>"#;
    let mut compiler = Compiler::new(resources()).unwrap();
    assert!(
        compiler.compile(xml).is_err(),
        "the default must stay strict"
    );
    assert!(
        compiler
            .set_fallback_font(Some("Not supplied".into()))
            .is_err()
    );
    compiler
        .set_fallback_font(Some("DejaVu Sans".into()))
        .unwrap();
    let doc = compiler.compile(xml).unwrap();
    assert!(!doc.scene().has_errors());
    let warnings = &doc.scene().diagnostics;
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].code, "font_substitution");
    assert_eq!(warnings[0].node_id, "t");
    assert!(warnings[0].message.contains("Missing Family"));
    assert!(warnings[0].source.is_some());
    let item = doc.scene().pages[0]
        .items
        .iter()
        .find(|item| item.id == "t")
        .unwrap();
    assert_eq!(item.text_style.family, "DejaVu Sans");
    assert_eq!(item.text_style.weight, 700);
    let Draw::Text { paragraphs, .. } = &item.kind else {
        panic!("expected text")
    };
    assert_eq!(paragraphs[0].spans[1].style["font-weight"], "400");
    let bytes = doc.export_pptx().unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut slide = String::new();
    zip.by_name("ppt/slides/slide1.xml")
        .unwrap()
        .read_to_string(&mut slide)
        .unwrap();
    assert!(
        !slide.contains("Missing Family"),
        "export must use the same resolved face as preview"
    );
    assert!(slide.contains("DejaVu Sans"));
    compiler.set_fallback_font(None).unwrap();
    assert!(compiler.compile(xml).is_err());
    assert!(
        doc.render_png(0, 1.0).is_ok(),
        "old document keeps resolved styles"
    );
}

#[test]
fn fallback_does_not_warn_for_exact_faces_or_synthesize_missing_italic() {
    let mut compiler = Compiler::new(resources()).unwrap();
    compiler
        .set_fallback_font(Some("DejaVu Sans".into()))
        .unwrap();
    let exact = compiler.compile(&source("Exact font")).unwrap();
    assert!(exact.scene().diagnostics.is_empty());
    let xml = source("Upright substitute").replace(
        "width:500px;",
        "font-style:italic;font-weight:700;width:500px;",
    );
    let doc = compiler.compile(&xml).unwrap();
    assert!(!doc.scene().has_errors());
    assert_eq!(doc.scene().diagnostics.len(), 2);
    for page in &doc.scene().pages {
        assert_eq!(page.items[0].text_style.family, "DejaVu Sans");
        assert_eq!(page.items[0].text_style.weight, 700);
        assert!(!page.items[0].text_style.italic);
    }
    assert!(doc.render_pdf().is_ok());
}

#[test]
fn font_fallback_does_not_hide_missing_glyphs() {
    let mut compiler = Compiler::new(resources()).unwrap();
    compiler
        .set_fallback_font(Some("DejaVu Sans".into()))
        .unwrap();
    let xml =
        source("\u{10ffff}").replace("font-family=\"DejaVu Sans\"", "font-family=\"Missing\"");
    let error = match compiler.compile(&xml) {
        Err(e) => e,
        Ok(_) => panic!("missing glyph must remain an error"),
    };
    assert!(error.to_string().contains("no glyph"));
}

#[test]
fn empty_text_has_no_line_box_but_authored_blank_lines_remain() {
    let mut compiler = Compiler::new(resources()).unwrap();
    let style = TextStyle {
        family: "DejaVu Sans".into(),
        ..TextStyle::default()
    };
    let empty = compiler
        .measure_text("", &style, Some(384.0), true)
        .unwrap();
    assert_eq!(
        (
            empty.width,
            empty.height,
            empty.line_count,
            empty.glyph_count
        ),
        (0.0, 0.0, 0, 0)
    );
    let blank_line = compiler
        .measure_text("\n", &style, Some(384.0), true)
        .unwrap();
    assert!(blank_line.height >= style.line_height);
    assert!(blank_line.line_count > 0);
    let xml = r#"<Deck font-family="DejaVu Sans"><Slide id="p"><Text id="empty" style="position:absolute;width:384px;height:0px"/><Text id="spans" style="position:absolute;top:50px;width:595.2px;height:0px"><Span/><Span/></Text></Slide></Deck>"#;
    let document = compiler.compile(xml).unwrap();
    assert!(document.scene().diagnostics.is_empty());
    for item in &document.scene().pages[0].items {
        let Draw::Text {
            glyphs, line_count, ..
        } = &item.kind
        else {
            panic!("expected empty text")
        };
        assert!(glyphs.is_empty());
        assert_eq!(*line_count, 0);
    }
    assert!(document.render_png(0, 1.0).is_ok());
    assert!(document.render_pdf().is_ok());
    assert!(document.export_pptx().is_ok());
    let visible = xml.replace("<Span/><Span/>", "Visible text");
    let document = compiler.compile(&visible).unwrap();
    assert!(
        document
            .scene()
            .diagnostics
            .iter()
            .any(|d| d.node_id == "spans" && d.code == "text_overflow")
    );
    assert!(document.render_png(0, 1.0).is_err());
}

#[test]
fn rgba_matches_png_including_straight_alpha_and_fractional_scale() {
    let xml = r##"<Deck width="81px" height="43px" font-family="DejaVu Sans"><Slide id="p" background="#20406040"><Shape id="s" shape="ellipse" style="position:absolute;left:5.5px;top:3.5px;width:40px;height:30px;background:#4080C080"/></Slide></Deck>"##;
    let doc = Compiler::new(resources()).unwrap().compile(xml).unwrap();
    for scale in [1.0, 0.7, 1.3333334] {
        let frame = doc.render_rgba(0, scale).unwrap();
        let png = image::load_from_memory(&doc.render_png(0, scale).unwrap())
            .unwrap()
            .to_rgba8();
        assert_eq!((frame.width, frame.height), png.dimensions());
        assert_eq!(frame.pixels, png.into_raw());
    }
    assert!(doc.render_rgba(1, 1.0).is_err());
    for scale in [0.0, -1.0, f32::NAN, f32::INFINITY, 4.01] {
        assert!(doc.render_rgba(0, scale).is_err());
    }
    assert!(
        doc.render_rgba(0, 1.0).unwrap().pixels[0] > 8,
        "channels must be unpremultiplied"
    );
}

#[test]
fn native_steps_share_snapshots_but_do_not_collide_with_static_page_cache() {
    let xml = r##"<Deck width="320px" height="180px" font-family="DejaVu Sans"><Slide id="p"><Shape id="a" style="position:absolute;left:20px;top:20px;width:60px;height:60px;background:#FF0000"/><Shape id="b" style="position:absolute;left:100px;top:20px;width:60px;height:60px;background:#0000FF"/><Animate target="b" effect="disappear" trigger="after-previous"/><Animate target="a" effect="appear"/><Animate target="b" effect="appear" trigger="with-previous"/><Animate target="a" effect="fill-color" color="#00FF00" trigger="after-previous"/><Animate target="a" effect="disappear"/></Slide></Deck>"##;
    let mut compiler = Compiler::new(resources()).unwrap();
    let doc = compiler.compile(xml).unwrap();
    drop(compiler);
    let snapshots = show::steps(&doc.scene().pages[0].show);
    assert_eq!(doc.step_count(0).unwrap(), 3);
    assert_eq!(doc.inspect()["pages"][0]["stepCount"], 3);
    assert!(snapshots[0]["a"].hidden && snapshots[0]["b"].hidden);
    assert_eq!(snapshots[1]["a"].fill, Some(Color(0, 255, 0, 255)));
    assert_ne!(
        doc.render_png(0, 1.0).unwrap(),
        doc.render_png_step(0, 0, 1.0).unwrap()
    );
    for (index, snapshot) in snapshots.iter().enumerate() {
        assert_eq!(
            &show::step(&doc.scene().pages[0].show, index).unwrap(),
            snapshot
        );
        let bytes = doc.render_png_step(0, index, 0.7).unwrap();
        let rgba = doc.render_rgba_step(0, index, 0.7).unwrap();
        assert_eq!(
            rgba.pixels,
            image::load_from_memory(&bytes)
                .unwrap()
                .to_rgba8()
                .into_raw()
        );
    }
    let hits = doc.render_stats().unwrap().png_cache_hits;
    doc.render_png_step(0, 0, 1.0).unwrap();
    assert_eq!(doc.render_stats().unwrap().png_cache_hits, hits + 1);
    assert!(doc.render_png_step(0, 3, 1.0).is_err());
    assert!(doc.render_rgba_step(0, usize::MAX, 1.0).is_err());
    assert!(doc.step_count(1).is_err());
    let invalid = Compiler::new(resources())
        .unwrap()
        .compile(&source("Too tall").replace("height:100px", "height:1px"))
        .unwrap();
    assert!(invalid.render_rgba_step(0, 0, 1.0).is_err());
}

#[test]
fn group_and_child_show_states_compose_visibility_opacity_and_motion() {
    let xml = r##"<Deck width="200px" height="200px" font-family="DejaVu Sans"><Slide id="p"><Group id="g" style="position:absolute;left:20px;top:20px;width:100px;height:100px"><Shape id="s" style="width:40px;height:40px;background:#FF0000"/></Group><Animate target="g" effect="motion-path" path="M 0 0 L 50 0"/><Animate target="s" effect="motion-path" path="M 0 0 L 0 50" trigger="with-previous"/><Animate target="g" effect="transparency" amount="0.5"/><Animate target="g" effect="disappear"/></Slide></Deck>"##;
    let doc = Compiler::new(resources()).unwrap().compile(xml).unwrap();
    let pixel = |step, x, y| {
        let frame = doc.render_rgba_step(0, step, 1.0).unwrap();
        let offset = ((y * frame.width + x) * 4) as usize;
        frame.pixels[offset..offset + 4].to_vec()
    };
    assert_eq!(pixel(1, 75, 75), [255, 0, 0, 255]);
    assert_eq!(pixel(1, 25, 75), [255, 255, 255, 255]);
    assert_eq!(pixel(2, 75, 75), [255, 127, 127, 255]);
    assert_eq!(pixel(3, 75, 75), [255, 255, 255, 255]);
}
