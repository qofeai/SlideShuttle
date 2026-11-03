mod io;
use clap::{Args, Parser, Subcommand};
use serde_json::json;
use slide_shuttle_core::{
    baseline::{self, Acceptance, Baseline},
    import::{import_pptx, inspect_pptx},
    *,
};
use std::{
    path::{Path, PathBuf},
    process::ExitCode,
};

#[derive(Parser)]
#[command(
    name = "slide-shuttle",
    version,
    about = "Independent native presentation compiler; no browser required"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Args)]
struct Input {
    input: PathBuf,
    #[arg(long)]
    font: Vec<PathBuf>,
}
#[derive(Args)]
struct ReviewArgs {
    #[arg(long)]
    base: Option<PathBuf>,
    #[arg(long)]
    against: Option<PathBuf>,
}
#[derive(Subcommand)]
enum Command {
    Measure {
        #[arg(long)]
        text: String,
        #[arg(long, default_value = "{}")]
        style: String,
        #[arg(long)]
        width: Option<f32>,
        #[arg(long)]
        nowrap: bool,
        #[arg(long, required = true)]
        font: Vec<PathBuf>,
    },
    Check {
        #[command(flatten)]
        input: Input,
        /// Check only the slide with this id; libraries and fonts still load.
        #[arg(long)]
        page: Option<String>,
    },
    Render {
        #[command(flatten)]
        input: Input,
        /// Render only the slide with this id.
        #[arg(long)]
        page: Option<String>,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        pdf: bool,
        #[arg(long, default_value_t = 1.0)]
        scale: f32,
    },
    Export {
        #[command(flatten)]
        input: Input,
        #[arg(long)]
        out: PathBuf,
        #[command(flatten)]
        review: ReviewArgs,
        #[arg(long)]
        accept: Option<PathBuf>,
        #[arg(long)]
        report: Option<PathBuf>,
    },
    Import {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        font: Vec<PathBuf>,
    },
    Inspect {
        input: PathBuf,
    },
    /// List Shape presets with their adjust values and defaults.
    Shapes,
    Diff {
        #[command(flatten)]
        input: Input,
        #[command(flatten)]
        review: ReviewArgs,
        #[arg(long)]
        out: Option<PathBuf>,
    },
}
fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
fn fail(s: impl Into<String>) -> Error {
    Error::Invalid(s.into())
}
fn print(v: &impl serde::Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}
/// Compile the project, or only one slide when `page` names a slide id. Also returns the
/// deck position of the first compiled slide, so output names match the full deck.
fn compile(input: &Input, page: Option<&str>) -> Result<(CompiledDocument, usize)> {
    let (mut deck, resources) = io::load(&input.input, &input.font)?;
    let mut first = 0;
    if let Some(page) = page {
        first = deck
            .slides
            .iter()
            .position(|s| s.id == page)
            .ok_or_else(|| fail(format!("no slide with id {page}")))?;
        deck.slides = vec![deck.slides.swap_remove(first)];
    }
    Ok((Compiler::new(resources)?.compile_deck(deck)?, first))
}
fn review(
    input: &Input,
    args: &ReviewArgs,
    d: &Deck,
    r: &Resources,
    planned: &import::Snapshot,
) -> Result<Option<baseline::Review>> {
    let default = input
        .input
        .parent()
        .unwrap_or(Path::new("."))
        .join(".slide-shuttle/base.json");
    let base = args
        .base
        .as_ref()
        .or_else(|| default.exists().then_some(&default));
    if args.against.is_some() && base.is_none() {
        return Err(fail(
            "--against requires --base (or the imported project's .slide-shuttle/base.json)",
        ));
    }
    let Some(base) = base else { return Ok(None) };
    let base: Baseline = serde_json::from_slice(&io::read(base, 64 * 1024 * 1024)?)?;
    let external = args
        .against
        .as_ref()
        .map(|p| io::read(p, 64 * 1024 * 1024).and_then(|b| inspect_pptx(&b)))
        .transpose()?;
    Ok(Some(baseline::review(
        &base,
        d,
        r,
        external.as_ref(),
        planned,
    )?))
}
fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Measure {
            text,
            style,
            width,
            nowrap,
            font,
        } => {
            if style.len() > 4096 {
                return Err(fail("measurement style exceeds budget"));
            }
            let style: TextStyle = serde_json::from_str(&style)?;
            let mut resources = Resources::default();
            for path in font {
                resources.fonts.push(FontResource {
                    family: None,
                    bytes: io::read(&path, 64 * 1024 * 1024)?,
                });
            }
            let mut compiler = Compiler::new(resources)?;
            print(&compiler.measure_text(&text, &style, width, !nowrap)?)?;
        }
        Command::Check { input, page } => {
            let (doc, _) = compile(&input, page.as_deref())?;
            let s = doc.scene();
            print(
                &json!({"valid":!s.has_errors(),"diagnostics":s.diagnostics,"pages":s.pages.iter().map(|p|json!({"id":p.id,"nodes":p.items.iter().map(|i|json!({"id":i.id,"rect":i.rect,"source":i.source,"lineCount":match &i.kind{Draw::Text{line_count,..}=>Some(*line_count),_=>None}})).collect::<Vec<_>>()})).collect::<Vec<_>>()}),
            )?;
            if s.has_errors() {
                return Err(fail("layout has errors"));
            }
        }
        Command::Render {
            input,
            out,
            pdf,
            scale,
            page,
        } => {
            let (doc, first) = compile(&input, page.as_deref())?;
            let (s, r) = (doc.scene(), doc.resources());
            if out.exists() {
                return Err(fail(
                    "render output directory already exists; choose a new path",
                ));
            }
            // Compute all output before publishing; a layout error cannot leave a partial deck.
            let png = (0..s.pages.len())
                .map(|i| render::render_png(s, i, r, scale))
                .collect::<Result<Vec<_>>>()?;
            let pdf = pdf.then(|| doc.render_pdf()).transpose()?;
            for (i, b) in png.iter().enumerate() {
                io::write_new(&out.join(format!("slide-{:03}.png", first + i + 1)), b)?;
            }
            if let Some(pdf) = pdf {
                io::write_new(&out.join("deck.pdf"), &pdf)?;
            }
            io::write_new(
                &out.join("manifest.json"),
                &serde_json::to_vec_pretty(
                    &json!({"engine":baseline::ENGINE,"resourceProfile":doc.resource_profile(),"pages":s.pages.len(),"width":s.width,"height":s.height,"diagnostics":s.diagnostics}),
                )?,
            )?;
            print(&json!({"output":out,"pages":s.pages.len(),"nativeRenderer":true}))?;
        }
        Command::Export {
            input,
            out,
            review: args,
            accept,
            report,
        } => {
            let (doc, _) = compile(&input, None)?;
            let b = doc.export_pptx()?;
            let snapshot = inspect_pptx(&b)?;
            let review = review(&input, &args, doc.deck(), doc.resources(), &snapshot)?;
            let acceptance = accept
                .as_ref()
                .map(|p| {
                    io::read(p, 4 * 1024 * 1024)
                        .and_then(|b| Ok(serde_json::from_slice::<Acceptance>(&b)?))
                })
                .transpose()?;
            if acceptance.is_some() && review.is_none() {
                return Err(fail("--accept requires a baseline and current diff"));
            }
            if let Some(v) = &review {
                if let Some(path) = report.as_ref() {
                    io::write_new(path, &serde_json::to_vec_pretty(v)?)?;
                }
                let unresolved = v.unresolved(acceptance.as_ref())?;
                if !unresolved.is_empty() {
                    print(v)?;
                    return Err(fail(format!(
                        "{} unresolved changes; update source or bind explicit decisions to diffId",
                        unresolved.len()
                    )));
                }
            }
            let base = Baseline::new(doc.deck().clone(), snapshot, doc.resource_profile().into())?;
            let dir = input
                .input
                .parent()
                .unwrap_or(Path::new("."))
                .join(".slide-shuttle/baselines")
                .join(&base.base_id);
            if out.exists() {
                return Err(fail("export writes a new file; output already exists"));
            }
            // Save recovery data first. Only then publish the recipient-facing PPTX.
            let saved = json!({"review":review,"acceptance":acceptance});
            if !dir.join("base.json").exists() {
                io::write_new(&dir.join("base.json"), &serde_json::to_vec_pretty(&base)?)?;
            }
            if let Some(v) = &review {
                let path = dir.join("reviews").join(format!("{}.json", v.diff_id));
                if !path.exists() {
                    io::write_new(&path, &serde_json::to_vec_pretty(&saved)?)?;
                }
            }
            let summary=review.as_ref().map(|v|json!({"diffId":v.diff_id,"externalDigest":v.external_digest,"acceptedIds":acceptance.as_ref().map(|a|a.items.keys().collect::<Vec<_>>()).unwrap_or_default()}));
            let receipt = out.with_extension("pptx.receipt.json");
            if receipt.exists() {
                return Err(fail("receipt already exists; choose a new output path"));
            }
            io::write_new(&out, &b)?;
            io::write_new(
                &receipt,
                &serde_json::to_vec_pretty(
                    &json!({"status":"complete","output":out,"sha256":digest(&b),"baseline":dir.join("base.json"),"externalChecked":args.against.is_some(),"decision":summary,"fontEmbedding":false}),
                )?,
            )?;
            print(
                &json!({"output":out,"baseline":dir.join("base.json"),"receipt":receipt,"externalChecked":args.against.is_some(),"diagnostics":doc.scene().diagnostics}),
            )?;
        }
        Command::Import { input, out, font } => {
            if out.exists() {
                return Err(fail("import output directory already exists"));
            }
            let imported = import_pptx(&io::read(&input, 64 * 1024 * 1024)?)?;
            let d = parse_files("deck.xml", &imported.files)?;
            let mut r = Resources::default();
            for f in font {
                r.fonts.push(FontResource {
                    family: None,
                    bytes: io::read(&f, 64 * 1024 * 1024)?,
                });
            }
            for (name, data) in &imported.assets {
                io::write_new(&out.join(name), data)?;
                r.images.insert(name.clone(), data.clone());
            }
            let base = Baseline::from_import(d, imported.snapshot.clone(), baseline::profile(&r)?)?;
            for (path, text) in &imported.files {
                io::write_new(&out.join(path), text.as_bytes())?;
            }
            io::write_new(
                &out.join(".slide-shuttle/base.json"),
                &serde_json::to_vec_pretty(&base)?,
            )?;
            io::write_new(
                &out.join("losses.json"),
                &serde_json::to_vec_pretty(&imported.snapshot.losses)?,
            )?;
            print(
                &json!({"output":out,"source":out.join("deck.xml"),"losses":imported.snapshot.losses,"note":"Import reconstructs supported objects into absolute layout; inspect losses before rebuilding"}),
            )?;
        }
        Command::Shapes => {
            print(
                &preset::catalog()
                    .into_iter()
                    .map(|(name, adjust)| {
                        json!({"name": name, "adjust": adjust.into_iter().map(|(k, v)| json!({"name": k, "default": v})).collect::<Vec<_>>()})
                    })
                    .collect::<Vec<_>>(),
            )?;
        }
        Command::Inspect { input } => {
            print(&inspect_pptx(&io::read(&input, 64 * 1024 * 1024)?)?)?;
        }
        Command::Diff {
            input,
            review: args,
            out,
        } => {
            let (doc, _) = compile(&input, None)?;
            let planned = inspect_pptx(&doc.export_pptx()?)?;
            let v = review(&input, &args, doc.deck(), doc.resources(), &planned)?
                .ok_or_else(|| fail("diff requires --base or imported .slide-shuttle/base.json"))?;
            if let Some(path) = out {
                io::write_new(&path, &serde_json::to_vec_pretty(&v)?)?;
            } else {
                print(&v)?;
            }
            if !v.unresolved(None)?.is_empty() {
                return Err(fail("diff has unresolved changes"));
            }
        }
    }
    Ok(())
}
