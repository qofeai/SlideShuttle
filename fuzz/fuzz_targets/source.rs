#![no_main]
//! Any source text must yield a value or an error, never a panic, through the whole pipeline.
use libfuzzer_sys::fuzz_target;
use slide_shuttle_core::{Compiler, FontResource, Resources};

fuzz_target!(|source: &str| {
    let resources = Resources {
        fonts: [
            &include_bytes!("../../crates/slide-shuttle-core/tests/fonts/DejaVuSans.ttf")[..],
            &include_bytes!("../../crates/slide-shuttle-core/tests/fonts/DejaVuSans-Bold.ttf")[..],
        ]
        .map(|bytes| FontResource {
            family: None,
            bytes: bytes.to_vec(),
        })
        .into(),
        images: Default::default(),
    };
    let Ok(doc) = Compiler::new(resources).unwrap().compile(source) else {
        return;
    };
    if !doc.scene().has_errors() {
        let _ = doc.render_png(0, 0.25);
        let _ = doc.export_pptx();
    }
});
