use serde_json::{Value, json};
use std::{
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn font() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../slide-shuttle-core/tests/fonts/DejaVuSans.ttf")
}
fn run(root: &Path, args: &[&str], with_font: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_slide-shuttle"));
    command.current_dir(root).args(args);
    if with_font {
        command.arg("--font").arg(font());
    }
    command.output().unwrap()
}
fn success(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn setup() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("pages")).unwrap();
    fs::write(dir.path().join("source.xml"),r#"<Deck width="640px" height="360px" font-family="DejaVu Sans"><Slide src="pages/s.xml"/></Deck>"#).unwrap();
    fs::write(dir.path().join("pages/s.xml"),r#"<Slide id="s"><Text id="t" style="width:500px;height:100px">Original body</Text></Slide>"#).unwrap();
    dir
}

fn packed_document(path: &Path) -> slide_shuttle_core::CompiledDocument {
    use slide_shuttle_core::{Compiler, package::Project};
    let mut project = Project::read(&fs::read(path).unwrap()).unwrap();
    let deck = project.deck().unwrap();
    Compiler::new(project.take_resources(&deck).unwrap())
        .unwrap()
        .compile_deck(deck)
        .unwrap()
}

#[test]
fn compact_fonts_preserve_shaping_generated_labels_and_declared_sources() {
    let root = setup();
    let p = root.path();
    fs::create_dir(p.join("fonts")).unwrap();
    fs::copy(font(), p.join("fonts/regular.ttf")).unwrap();
    fs::copy(
        font().with_file_name("DejaVuSans-Bold.ttf"),
        p.join("fonts/bold.ttf"),
    )
    .unwrap();
    let source = r#"<Deck width="1000px" height="800px" font-family="DejaVu Sans"><Library src="design.xml"/><Slide src="pages/s.xml"/></Deck>"#;
    let library = r#"<Library><!-- keep author comments --><Font family="DejaVu Sans" src="./fonts/regular.ttf"/><Font family="DejaVu Sans" src="fonts/bold.ttf"/></Library>"#;
    fs::write(p.join("source.xml"), source).unwrap();
    fs::write(p.join("design.xml"), library).unwrap();
    fs::write(p.join("pages/s.xml"), r#"<Slide id="s">
      <Text id="text" style="font-size:24px;width:900px;height:90px">office ffi café é Ελληνικά العربية <Span style="font-weight:700">Bold</Span></Text>
      <Text id="list" style="position:absolute;left:0px;top:100px;width:900px;height:100px;font-size:24px"><List><Item>List item</Item><Item>Second</Item></List></Text>
      <Table id="table" columns="1 1" style="position:absolute;top:220px;width:800px;font-size:24px"><Row><Cell>Table label</Cell><Cell>42</Cell></Row></Table>
      <Chart id="chart" type="bar" min="0" max="10" style="position:absolute;top:320px;width:800px;height:350px;font-size:20px"><Labels>A|B</Labels><Series name="Series">2 7</Series></Chart>
      <Animate target="text" effect="appear"/>
    </Slide>"#).unwrap();
    let report = success(run(
        p,
        &["pack", "source.xml", "--out", "compact.qppt"],
        false,
    ));
    assert_eq!(report["fonts"]["mode"], "subset");
    assert_eq!(report["fonts"]["faces"], 2);
    assert!(report["fonts"]["embeddedBytes"].as_u64().unwrap() < 100_000);
    success(run(
        p,
        &["pack", "source.xml", "--out", "full.qppt", "--full-fonts"],
        false,
    ));
    let original = packed_document(&p.join("full.qppt"));
    let compact = packed_document(&p.join("compact.qppt"));
    assert_eq!(
        original.render_png(0, 1.0).unwrap(),
        compact.render_png(0, 1.0).unwrap()
    );
    for step in 0..original.step_count(0).unwrap() {
        assert_eq!(
            original.render_png_step(0, step, 1.0).unwrap(),
            compact.render_png_step(0, step, 1.0).unwrap()
        );
    }
    assert_eq!(fs::read_to_string(p.join("source.xml")).unwrap(), source);
    assert_eq!(fs::read_to_string(p.join("design.xml")).unwrap(), library);
    let package =
        slide_shuttle_core::package::Project::read(&fs::read(p.join("compact.qppt")).unwrap())
            .unwrap();
    assert!(package.files["design.xml"].contains("keep author comments"));
    assert!(!package.files["design.xml"].contains("<Font"));
    assert!(!package.assets.contains_key("fonts/regular.ttf"));
    assert_eq!(
        original.resources().fonts[0].bytes,
        fs::read(font()).unwrap()
    );
}

#[test]
fn compact_fonts_drop_unused_weight_and_support_shapes_only() {
    let root = setup();
    let p = root.path();
    let bold = font().with_file_name("DejaVuSans-Bold.ttf");
    let report = success(run(
        p,
        &[
            "pack",
            "source.xml",
            "--out",
            "regular.qppt",
            "--font",
            bold.to_str().unwrap(),
        ],
        true,
    ));
    assert_eq!(report["fonts"]["faces"], 1);
    assert!(report["bytes"].as_u64().unwrap() < 30_000);
    fs::write(p.join("pages/s.xml"), r#"<Slide id="s"><Shape id="box" style="width:100px;height:100px;background:#ff0000"/></Slide>"#).unwrap();
    success(run(
        p,
        &["pack", "source.xml", "--out", "shapes.qppt"],
        true,
    ));
    assert!(
        packed_document(&p.join("shapes.qppt"))
            .render_png(0, 1.0)
            .is_ok()
    );
}

#[test]
fn skill_delivers_only_requested_formats_and_keeps_review_internal() {
    let root = setup();
    let p = root.path();
    // Four meaningful colours and no claim title are valid design choices, even
    // when readability warnings are explicitly made blocking.
    fs::write(p.join("pages/s.xml"), r##"<Slide id="s" background="#ffffff"><Text id="content" style="font-size:24px;line-height:32px;width:600px;height:80px"><Span style="color:#993333">A</Span><Span style="color:#337733">B</Span><Span style="color:#333399">C</Span><Span style="color:#773377">D</Span></Text></Slide>"##).unwrap();
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/slideshuttle/scripts/build.py");
    for format in [None, Some("pdf"), Some("pptx")] {
        let out = p.join(format.unwrap_or("default"));
        let mut cmd = Command::new("python3");
        cmd.arg(&script)
            .arg(p.join("source.xml"))
            .arg("--out")
            .arg(&out)
            .arg("--cli")
            .arg(env!("CARGO_BIN_EXE_slide-shuttle"))
            .arg("--font")
            .arg(font())
            .arg("--strict-design");
        if let Some(format) = format {
            cmd.arg("--export").arg(format);
        }
        let result = success(cmd.output().unwrap());
        let files = result["deliverables"].as_array().unwrap();
        assert_eq!(files.len(), if format.is_some() { 2 } else { 1 });
        assert!(
            files[0]
                .as_str()
                .unwrap()
                .ends_with("/deliverables/presentation.qppt")
        );
        assert!(out.join("preview/slide-001.png").is_file());
        assert!(result["review"]["previews"].is_array());
        assert!(!out.join("preview/deck.pdf").exists());
        assert_eq!(
            out.join("deliverables/presentation.pdf").exists(),
            format == Some("pdf")
        );
        assert_eq!(
            out.join("deliverables/presentation.pptx").exists(),
            format == Some("pptx")
        );
        assert_eq!(
            fs::read_dir(out.join("deliverables")).unwrap().count(),
            files.len()
        );
        assert!(
            result["designAudit"]["hints"]
                .as_array()
                .unwrap()
                .iter()
                .any(|h| h["code"] == "palette_review")
        );
    }
}

#[test]
fn cli_bare_path_native_outputs_and_refuses_existing_file() {
    let root = setup();
    let p = root.path();
    assert!(!run(p, &["check", "source.xml"], false).status.success());
    assert!(
        success(run(p, &["check", "source.xml"], true))["valid"]
            .as_bool()
            .unwrap()
    );
    success(run(
        p,
        &["render", "source.xml", "--out", "preview", "--pdf"],
        true,
    ));
    assert!(p.join("preview/slide-001.png").exists());
    assert!(p.join("preview/deck.pdf").exists());
    assert!(
        !run(p, &["render", "source.xml", "--out", "preview"], true)
            .status
            .success()
    );
    let exported = success(run(
        p,
        &["export", "source.xml", "--out", "first.pptx"],
        true,
    ));
    assert_eq!(exported["externalChecked"], false);
    let before = fs::read(p.join("first.pptx")).unwrap();
    assert!(
        !run(p, &["export", "source.xml", "--out", "first.pptx"], true)
            .status
            .success()
    );
    assert_eq!(fs::read(p.join("first.pptx")).unwrap(), before);
    let base = exported["baseline"].as_str().unwrap();
    let review = success(run(
        p,
        &[
            "diff",
            "source.xml",
            "--base",
            base,
            "--against",
            "first.pptx",
        ],
        true,
    ));
    assert!(review["changes"].as_array().unwrap().is_empty());
    assert!(
        !run(p, &["diff", "source.xml", "--against", "first.pptx"], true)
            .status
            .success()
    );
}

#[test]
fn import_rebuild_requires_specific_current_decisions() {
    let root = setup();
    let p = root.path();
    success(run(
        p,
        &["export", "source.xml", "--out", "original.pptx"],
        true,
    ));
    let imported = success(run(
        p,
        &["import", "original.pptx", "--out", "imported"],
        true,
    ));
    assert!(p.join("imported/.slide-shuttle/base.json").exists());
    assert!(imported["losses"].as_array().unwrap().is_empty());
    success(run(p, &["check", "imported/deck.xml"], true));
    let failed = run(
        p,
        &[
            "export",
            "imported/deck.xml",
            "--out",
            "rebuilt.pptx",
            "--report",
            "review.json",
        ],
        true,
    );
    assert!(!failed.status.success());
    assert!(!p.join("rebuilt.pptx").exists());
    let review: Value = serde_json::from_slice(&fs::read(p.join("review.json")).unwrap()).unwrap();
    let items=review["changes"].as_array().unwrap().iter().filter(|c|c["blocking"]==true).map(|c|(c["id"].as_str().unwrap().to_owned(),json!("Rebuild into a new static deck; retain source text, accept changed object identity"))).collect::<serde_json::Map<_,_>>();
    assert!(!items.is_empty());
    let acceptance = json!({"diffId":review["diffId"],"items":items});
    fs::write(
        p.join("decisions.json"),
        serde_json::to_vec(&acceptance).unwrap(),
    )
    .unwrap();
    success(run(
        p,
        &[
            "export",
            "imported/deck.xml",
            "--out",
            "rebuilt.pptx",
            "--accept",
            "decisions.json",
        ],
        true,
    ));
    let receipt = fs::read_to_string(p.join("rebuilt.pptx.receipt.json")).unwrap();
    assert!(!receipt.contains("Original body"));
    assert!(!receipt.contains("\"changes\""));
    let page = p.join("imported/pages/page-1.xml");
    let source = fs::read_to_string(&page)
        .unwrap()
        .replace("Original body", "Changed body");
    fs::write(page, source).unwrap();
    assert!(
        !run(
            p,
            &[
                "export",
                "imported/deck.xml",
                "--out",
                "stale.pptx",
                "--accept",
                "decisions.json"
            ],
            true
        )
        .status
        .success()
    );
    assert!(!p.join("stale.pptx").exists());
}

#[test]
fn actual_external_edit_and_animation_block_export() {
    let root = setup();
    let p = root.path();
    let exported = success(run(
        p,
        &["export", "source.xml", "--out", "original.pptx"],
        true,
    ));
    let base = exported["baseline"].as_str().unwrap();
    let data = fs::read(p.join("original.pptx")).unwrap();
    let mut input = zip::ZipArchive::new(Cursor::new(data)).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..input.len() {
        let mut file = input.by_index(i).unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        if file.name() == "ppt/slides/slide1.xml" {
            bytes = String::from_utf8(bytes)
                .unwrap()
                .replace("Original body", "Externally changed")
                .replace("</p:sld>", "<p:timing/></p:sld>")
                .into_bytes();
        }
        output
            .start_file(file.name(), zip::write::SimpleFileOptions::default())
            .unwrap();
        output.write_all(&bytes).unwrap();
    }
    fs::write(
        p.join("external.pptx"),
        output.finish().unwrap().into_inner(),
    )
    .unwrap();
    let diff = run(
        p,
        &[
            "diff",
            "source.xml",
            "--base",
            base,
            "--against",
            "external.pptx",
            "--out",
            "review.json",
        ],
        true,
    );
    assert!(!diff.status.success());
    let report: Value = serde_json::from_slice(&fs::read(p.join("review.json")).unwrap()).unwrap();
    let changes = report["changes"].as_array().unwrap();
    assert!(
        changes
            .iter()
            .any(|c| c["property"] == "text" && c["status"] == "external_unresolved")
    );
    assert!(
        changes
            .iter()
            .any(|c| c["property"] == "packagePart" && c["blocking"] == true)
    );
    assert!(
        !run(
            p,
            &[
                "export",
                "source.xml",
                "--out",
                "lost.pptx",
                "--base",
                base,
                "--against",
                "external.pptx"
            ],
            true
        )
        .status
        .success()
    );
    assert!(!p.join("lost.pptx").exists());
}

#[test]
fn declared_resource_cannot_escape_project_root() {
    let root = setup();
    fs::write(
        root.path().join("source.xml"),
        r#"<Deck><Font family="DejaVu Sans" src="../outside.ttf"/><Slide src="pages/s.xml"/></Deck>"#,
    )
    .unwrap();
    let result = run(root.path(), &["check", "source.xml"], false);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("relative paths"));
}

#[test]
fn standalone_measurement_reports_actual_font_geometry() {
    let root = setup();
    let style = r#"{"family":"DejaVu Sans","size":24,"lineHeight":34}"#;
    let wide = success(run(
        root.path(),
        &[
            "measure",
            "--text",
            "A paragraph with several words",
            "--style",
            style,
        ],
        true,
    ));
    let narrow = success(run(
        root.path(),
        &[
            "measure",
            "--text",
            "A paragraph with several words",
            "--style",
            style,
            "--width",
            "90",
        ],
        true,
    ));
    assert_eq!(wide["lineCount"], 1);
    assert!(narrow["lineCount"].as_u64().unwrap() > 1);
    assert!(
        !run(
            root.path(),
            &["measure", "--text", "x", "--style", r#"{"fontSize":24}"#],
            true
        )
        .status
        .success()
    );
}

#[test]
fn cli_loads_multi_file_projects_and_checks_one_page() {
    let root = tempfile::tempdir().unwrap();
    let p = root.path();
    fs::create_dir_all(p.join("pages")).unwrap();
    fs::write(p.join("deck.xml"), r#"<Deck width="640px" height="360px" font-family="DejaVu Sans"><Library src="system.xml"/><Slide src="pages/a.xml"/><Slide src="pages/b.xml"/></Deck>"#).unwrap();
    fs::write(
        p.join("system.xml"),
        r#"<Library><Style name="t">width:500px;height:60px</Style></Library>"#,
    )
    .unwrap();
    fs::write(
        p.join("pages/a.xml"),
        r#"<Slide id="a"><Text id="t" class="t">A</Text></Slide>"#,
    )
    .unwrap();
    fs::write(p.join("pages/b.xml"), r#"<Slide id="b"><Text id="t" class="t" style="width:9px">Too long for this</Text></Slide>"#).unwrap();
    assert!(
        success(run(p, &["check", "deck.xml", "--page", "a"], true))["valid"]
            .as_bool()
            .unwrap()
    );
    let failed = run(p, &["check", "deck.xml"], true);
    assert!(!failed.status.success());
    let report: Value = serde_json::from_slice(&failed.stdout).unwrap();
    assert_eq!(report["diagnostics"][0]["source"]["file"], "pages/b.xml");
    success(run(
        p,
        &["render", "deck.xml", "--page", "a", "--out", "one"],
        true,
    ));
    fs::write(
        p.join("pages/b.xml"),
        r#"<Slide id="b"><Text id="t" class="t">B</Text></Slide>"#,
    )
    .unwrap();
    success(run(
        p,
        &["render", "deck.xml", "--page", "b", "--out", "two"],
        true,
    ));
    assert!(p.join("one/slide-001.png").exists() && p.join("two/slide-002.png").exists());
    fs::write(
        p.join("deck.xml"),
        r#"<Deck><Slide src="../outside.xml"/></Deck>"#,
    )
    .unwrap();
    let escaped = run(p, &["check", "deck.xml"], true);
    assert!(
        String::from_utf8_lossy(&escaped.stderr)
            .contains("references must be relative paths inside the project")
    );
}

#[test]
fn cli_step_manifest_names_the_native_states_and_failed_render_leaves_no_output() {
    let root = setup();
    let p = root.path();
    let source = fs::read_to_string(p.join("pages/s.xml")).unwrap().replace(
        "</Slide>",
        r#"<Animate target="t" effect="appear"/></Slide>"#,
    );
    fs::write(p.join("pages/s.xml"), source).unwrap();
    success(run(
        p,
        &["render", "source.xml", "--out", "steps", "--steps"],
        true,
    ));
    let manifest: Value =
        serde_json::from_slice(&fs::read(p.join("steps/manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["slides"][0]["stepCount"], 2);
    assert_eq!(manifest["slides"][0]["stepsExported"], true);
    let png = |name| fs::read(p.join(format!("steps/{name}"))).unwrap();
    assert_ne!(png("slide-001.png"), png("slide-001-step-0.png"));
    assert_eq!(png("slide-001.png"), png("slide-001-step-1.png"));
    assert!(
        !run(
            p,
            &["render", "source.xml", "--out", "failed", "--scale", "5"],
            true
        )
        .status
        .success()
    );
    assert!(!p.join("failed").exists());
    assert!(!fs::read_dir(p).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".slide-shuttle-render-")
    }));
}

#[test]
fn selecting_a_page_loads_only_its_media_and_keeps_original_page_number() {
    let root = setup();
    let p = root.path();
    fs::write(p.join("source.xml"), r#"<Deck font-family="DejaVu Sans"><Slide src="pages/unfinished.xml"/><Slide src="pages/ready.xml"/></Deck>"#).unwrap();
    fs::write(
        p.join("pages/unfinished.xml"),
        r#"<Slide id="unfinished"><Image id="missing" src="not-authored-yet.png"/></Slide>"#,
    )
    .unwrap();
    fs::write(p.join("pages/ready.xml"), r#"<Slide id="ready"><Text id="title" style="width:500px;height:100px">Work on one page</Text></Slide>"#).unwrap();
    assert!(!run(p, &["check", "source.xml"], true).status.success());
    assert_eq!(
        success(run(p, &["check", "source.xml", "--page", "ready"], true))["valid"],
        true
    );
    success(run(
        p,
        &[
            "render",
            "source.xml",
            "--page",
            "ready",
            "--out",
            "one-page",
        ],
        true,
    ));
    assert!(p.join("one-page/slide-002.png").exists());
    assert!(!p.join("one-page/slide-001.png").exists());
    assert!(
        !run(p, &["check", "source.xml", "--page", "unknown"], true)
            .status
            .success()
    );
}

#[test]
fn qppt_pack_unpack_and_render_are_portable_and_preserve_private_boundaries() {
    let root = setup();
    let p = root.path();
    fs::create_dir_all(p.join(".slide-shuttle")).unwrap();
    fs::write(p.join(".slide-shuttle/private.txt"), "private history").unwrap();
    success(run(
        p,
        &["pack", "source.xml", "--out", "presentation.qppt"],
        true,
    ));
    let inspection = success(run(p, &["inspect", "presentation.qppt"], false));
    assert_eq!(inspection["format"], "qppt");
    assert!(inspection["manifest"].get("version").is_none());
    assert_eq!(inspection["sources"].as_array().unwrap().len(), 2);
    assert_eq!(
        success(run(p, &["check", "presentation.qppt"], false))["valid"],
        true
    );
    success(run(
        p,
        &["render", "presentation.qppt", "--out", "packed-preview"],
        false,
    ));
    success(run(
        p,
        &["render", "source.xml", "--out", "source-preview"],
        true,
    ));
    assert_eq!(
        fs::read(p.join("packed-preview/slide-001.png")).unwrap(),
        fs::read(p.join("source-preview/slide-001.png")).unwrap()
    );
    success(run(
        p,
        &["unpack", "presentation.qppt", "--out", "unpacked"],
        false,
    ));
    assert!(!p.join("unpacked/.slide-shuttle").exists());
    assert_eq!(
        success(run(p, &["check", "unpacked"], false))["valid"],
        true
    );
    success(run(p, &["pack", "unpacked", "--out", "again.qppt"], false));
    assert_eq!(
        fs::read(p.join("presentation.qppt")).unwrap(),
        fs::read(p.join("again.qppt")).unwrap()
    );
    assert!(
        !run(
            p,
            &["unpack", "presentation.qppt", "--out", "unpacked"],
            false
        )
        .status
        .success()
    );
    assert!(
        !run(
            p,
            &["pack", "source.xml", "--out", "presentation.qppt"],
            true
        )
        .status
        .success()
    );
    fs::write(p.join("inline.xml"), r#"<Deck><Slide id="p"/></Deck>"#).unwrap();
    assert!(
        !run(p, &["pack", "inline.xml", "--out", "invalid.qppt"], true)
            .status
            .success()
    );
    assert!(!run(p, &["check", "inline.xml"], true).status.success());
    assert!(!p.join("invalid.qppt").exists());
}

#[test]
fn qppt_does_not_erase_import_rebuild_requirements() {
    let root = setup();
    let p = root.path();
    success(run(
        p,
        &["export", "source.xml", "--out", "original.pptx"],
        true,
    ));
    success(run(
        p,
        &["import", "original.pptx", "--out", "imported"],
        true,
    ));
    success(run(
        p,
        &["pack", "imported", "--out", "imported.qppt"],
        true,
    ));
    assert_eq!(
        success(run(p, &["inspect", "imported.qppt"], false))["manifest"]["imported"],
        true
    );
    assert!(
        !run(p, &["export", "imported.qppt", "--out", "lost.pptx"], false)
            .status
            .success()
    );
    success(run(
        p,
        &["unpack", "imported.qppt", "--out", "portable-import"],
        false,
    ));
    assert!(
        !run(
            p,
            &[
                "export",
                "portable-import/deck.xml",
                "--out",
                "lost-again.pptx"
            ],
            false
        )
        .status
        .success()
    );
    assert!(!p.join("lost.pptx").exists());
}
