#![no_main]
//! Structure-aware PPTX fuzzing: replace slide XML inside a valid package, so mutations
//! reach the OOXML reader instead of failing ZIP CRC checks.
use libfuzzer_sys::fuzz_target;
use slide_shuttle_core::{Compiler, FontResource, Resources, import::import_pptx};
use std::{
    io::{Cursor, Read, Write},
    sync::OnceLock,
};

fn base() -> &'static Vec<(String, Vec<u8>)> {
    static BASE: OnceLock<Vec<(String, Vec<u8>)>> = OnceLock::new();
    BASE.get_or_init(|| {
        let resources = Resources {
            fonts: vec![FontResource {
                family: None,
                bytes: include_bytes!("../../crates/slide-shuttle-core/tests/fonts/DejaVuSans.ttf")
                    .to_vec(),
            }],
            images: Default::default(),
        };
        let source = r##"<Deck version="1" width="640px" height="360px" font-family="DejaVu Sans"><Slide id="s"><Text id="t" style="width:300px;height:40px">Hi</Text><Shape id="r" style="width:20px;height:20px;background:#09A88F"/></Slide></Deck>"##;
        let pptx = Compiler::new(resources).unwrap().compile(source).unwrap().export_pptx().unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(pptx)).unwrap();
        (0..zip.len())
            .map(|i| {
                let mut f = zip.by_index(i).unwrap();
                let mut bytes = Vec::new();
                f.read_to_end(&mut bytes).unwrap();
                (f.name().to_owned(), bytes)
            })
            .collect()
    })
}

fuzz_target!(|slide: &[u8]| {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in base() {
        writer
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        let bytes = if name == "ppt/slides/slide1.xml" {
            slide
        } else {
            bytes
        };
        writer.write_all(bytes).unwrap();
    }
    let package = writer.finish().unwrap().into_inner();
    if let Ok(imported) = import_pptx(&package) {
        let _ = slide_shuttle_core::parse(&imported.xml);
    }
});
