//! Run with: cargo run --release -p slide-shuttle-core --example render_benchmark
use slide_shuttle_core::{Compiler, FontResource, Resources, render};
use std::{hint::black_box, time::Instant};

fn main() {
    let iterations = std::env::args()
        .nth(1)
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(20)
        .max(1);
    let resources = Resources {
        fonts: vec![FontResource {
            family: Some("DejaVu Sans".into()),
            bytes: include_bytes!("../tests/fonts/DejaVuSans.ttf").to_vec(),
        }],
        ..Resources::default()
    };
    let rows = (0..12).map(|i| format!(r#"<Text id="t{i}" style="position:absolute;left:40px;top:{}px;width:1200px;height:42px;font-size:24px;line-height:32px">SlideShuttle native rendering: repeated glyphs, clear text, page {i}.</Text>"#, 30 + i * 54)).collect::<String>();
    let source = format!(
        r#"<Deck width="1280px" height="720px" font-family="DejaVu Sans"><Slide id="page">{rows}<Animate target="t0" effect="fade-in"/></Slide></Deck>"#
    );
    let start = Instant::now();
    let document = Compiler::new(resources).unwrap().compile(&source).unwrap();
    let compile_ms = start.elapsed().as_secs_f64() * 1000.0;
    let warm = document.render_png(0, 1.0).unwrap();
    let start = Instant::now();
    for _ in 0..iterations {
        black_box(render::render_png(document.scene(), 0, document.resources(), 1.0).unwrap());
    }
    let png_ms = start.elapsed().as_secs_f64() * 1000.0 / iterations as f64;
    let start = Instant::now();
    for _ in 0..iterations {
        black_box(document.render_png(0, 1.0).unwrap());
    }
    let cache_ms = start.elapsed().as_secs_f64() * 1000.0 / iterations as f64;
    let start = Instant::now();
    for _ in 0..iterations {
        black_box(document.render_rgba(0, 1.0).unwrap());
    }
    let rgba_ms = start.elapsed().as_secs_f64() * 1000.0 / iterations as f64;
    println!(
        "{}",
        serde_json::json!({"iterations":iterations,"width":1280,"height":720,"compileMs":compile_ms,"pngMs":png_ms,"rgbaMs":rgba_ms,"cachedPngMs":cache_ms,"pngBytes":warm.len(),"rgbaBytes":1280*720*4,"pngSha256":slide_shuttle_core::digest(&warm)})
    );
}
