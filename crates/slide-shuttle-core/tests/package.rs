use slide_shuttle_core::{
    package::{Manifest, Project},
    *,
};
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
};

fn input() -> BTreeMap<String, Vec<u8>> {
    BTreeMap::from([
        ("deck.xml".into(), br#"<Deck width="320px" height="180px" font-family="DejaVu Sans"><Font family="DejaVu Sans" src="fonts/regular.ttf"/><Library src="design.xml"/><Asset id="picture" src="media/palette.png"/><Slide src="pages/intro.xml"/></Deck>"#.to_vec()),
        ("design.xml".into(), br#"<Library><Style name="title">width:280px;height:50px;font-size:20px</Style></Library>"#.to_vec()),
        ("pages/intro.xml".into(), br#"<Slide id="intro"><Text id="title" class="title">Native package</Text><Image id="image" src="picture" style="width:120px;height:80px"/><Animate target="image" effect="appear"/></Slide>"#.to_vec()),
        ("fonts/regular.ttf".into(), include_bytes!("fonts/DejaVuSans.ttf").to_vec()),
        ("media/palette.png".into(), include_bytes!("../../../examples/palette.png").to_vec()),
        (".slide-shuttle/private.txt".into(), b"history must stay private".to_vec()),
        ("unused.png".into(), b"unreferenced file".to_vec()),
    ])
}
fn project() -> Project {
    let input = input();
    Project::load("deck.xml", Manifest::default(), |name| {
        input
            .get(name)
            .cloned()
            .ok_or_else(|| Error::Invalid(format!("missing {name}")))
    })
    .unwrap()
}
fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn qppt_is_self_contained_deterministic_and_has_no_version_field() {
    let project = project();
    assert_eq!(project.files.len(), 3);
    assert_eq!(project.assets.len(), 2);
    let deck = project.deck().unwrap();
    let expected = Compiler::new(project.resources(&deck).unwrap())
        .unwrap()
        .compile_deck(deck)
        .unwrap();
    let bytes = project.write().unwrap();
    assert_eq!(project.write().unwrap(), bytes);
    let mut archive = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_reader(archive.by_name("manifest.json").unwrap()).unwrap();
    assert_eq!(
        manifest,
        serde_json::json!({"format":"qppt","entry":"deck.xml"})
    );
    assert!(archive.by_name(".slide-shuttle/private.txt").is_err());
    assert!(archive.by_name("unused.png").is_err());
    let mut loaded = Project::read(&bytes).unwrap();
    let deck = loaded.deck().unwrap();
    let pointer = loaded.assets["fonts/regular.ttf"].as_ptr();
    let resources = loaded.take_resources(&deck).unwrap();
    assert_eq!(
        pointer,
        resources.fonts[0].bytes.as_ptr(),
        "moving a font must not duplicate its allocation"
    );
    assert!(loaded.assets.is_empty());
    let actual = Compiler::new(resources)
        .unwrap()
        .compile_deck(deck)
        .unwrap();
    assert_eq!(
        actual.render_png(0, 1.0).unwrap(),
        expected.render_png(0, 1.0).unwrap()
    );
    assert_eq!(
        actual.render_png_step(0, 0, 1.0).unwrap(),
        expected.render_png_step(0, 0, 1.0).unwrap()
    );
}

#[test]
fn explicit_fonts_are_embedded_and_deduplicated() {
    let mut project = project();
    let font = include_bytes!("fonts/DejaVuSans-Bold.ttf").to_vec();
    project.add_font(font.clone()).unwrap();
    project.add_font(font).unwrap();
    assert_eq!(project.manifest.fonts.len(), 1);
    let mut loaded = Project::read(&project.write().unwrap()).unwrap();
    let deck = loaded.deck().unwrap();
    assert_eq!(loaded.take_resources(&deck).unwrap().fonts.len(), 2);
}

#[test]
fn packages_require_external_pages_and_referenced_resources() {
    let source = b"<Deck><Slide id=\"p\"/></Deck>";
    let error = Project::load("deck.xml", Manifest::default(), |_| Ok(source.to_vec()))
        .err()
        .unwrap();
    assert!(error.to_string().contains("multi-file project"));
    let mut project = project();
    project.assets.remove("media/palette.png");
    assert!(project.write().is_err());
    assert!(
        Project::read(&zip(&[(
            "manifest.json",
            br#"{"format":"qppt","entry":"deck.xml"}"#
        )]))
        .is_err()
    );
    assert!(
        Project::read(&zip(&[(
            "manifest.json",
            br#"{"format":"pptx","entry":"deck.xml"}"#
        )]))
        .is_err()
    );
}

#[test]
fn zip_paths_duplicates_and_symlinks_are_rejected_before_extraction() {
    for name in [
        "../outside",
        "/absolute",
        "C:/drive",
        "pages\\bad",
        "./deck.xml",
        "a//b",
    ] {
        let error = Project::read(&zip(&[(name, b"x")]))
            .err()
            .unwrap()
            .to_string();
        assert!(
            error.contains("relative paths") || error.contains("QPPT ZIP path"),
            "wrong rejection for {name}: {error}"
        );
    }
    assert!(
        Project::read(&zip(&[("deck.xml", b"x"), ("DECK.xml", b"y")]))
            .err()
            .unwrap()
            .to_string()
            .contains("duplicate")
    );
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    archive
        .add_symlink(
            "escape",
            "../outside",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    assert!(
        Project::read(&archive.finish().unwrap().into_inner())
            .err()
            .unwrap()
            .to_string()
            .contains("symlinks")
    );
    let mut project = project();
    project.assets.insert("pages".into(), vec![]);
    assert!(project.write().is_err());
}

#[test]
fn imported_origin_and_loss_metadata_survive_packaging() {
    let mut project = project();
    project.manifest.imported = true;
    project.manifest.losses.push(import::Loss {
        page: "intro".into(),
        object: "unknown".into(),
        reason: "unsupported object".into(),
    });
    let loaded = Project::read(&project.write().unwrap()).unwrap();
    assert!(loaded.manifest.imported);
    assert_eq!(loaded.manifest.losses.len(), 1);
    project.manifest.imported = false;
    assert!(project.write().is_err());
}

#[test]
fn package_limits_are_checked_before_resource_duplication() {
    let oversized = vec![b' '; 1024 * 1024 + 1];
    assert!(
        Project::read(&zip(&[("manifest.json", &oversized)]))
            .err()
            .unwrap()
            .to_string()
            .contains("budget")
    );
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..4097 {
        archive
            .start_file(
                format!("entry-{index}"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
    }
    assert!(
        Project::read(&archive.finish().unwrap().into_inner())
            .err()
            .unwrap()
            .to_string()
            .contains("too many entries")
    );
    let mut project = project();
    let mut deck = project.deck().unwrap();
    deck.fonts = vec![deck.fonts[0].clone(); 1000];
    assert!(
        project
            .take_resources(&deck)
            .err()
            .unwrap()
            .to_string()
            .contains("resource budget")
    );
    assert_eq!(
        project.assets.len(),
        2,
        "an excessive duplicate resource plan must fail before moving/cloning bytes"
    );
}
