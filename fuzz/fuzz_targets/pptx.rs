#![no_main]
//! Untrusted PPTX bytes must be rejected or imported, never panic or exhaust memory.
use libfuzzer_sys::fuzz_target;
use slide_shuttle_core::import::{import_pptx, inspect_pptx};

fuzz_target!(|bytes: &[u8]| {
    let _ = inspect_pptx(bytes);
    if let Ok(imported) = import_pptx(bytes) {
        let _ = slide_shuttle_core::parse_files("deck.xml", &imported.files);
    }
});
