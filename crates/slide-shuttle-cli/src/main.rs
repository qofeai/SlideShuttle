mod fonts;
mod io;
use clap::{Args, Parser, Subcommand};
use serde_json::json;
use slide_shuttle_core::{
    baseline::{self, Acceptance, Baseline},
    import::{import_pptx, inspect_pptx},
    *,
};
use std::{path::PathBuf, process::ExitCode};

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
    /// Package a multi-file native project and its referenced resources into .qppt.
    Pack {
        #[command(flatten)]
        input: Input,
        #[arg(long)]
        out: PathBuf,
        /// Keep supplied font files unchanged for offline editing with new characters.
        /// By default only used faces, characters and layout dependencies are embedded.
        #[arg(long)]
        full_fonts: bool,
    },
    /// Extract a QPPT into a new editable project directory.
    Unpack {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
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
        /// Also draw animated slides before the first click and after each click.
        #[arg(long)]
        steps: bool,
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
fn compile(input: &Input, page: Option<&str>) -> Result<(CompiledDocument, usize, bool)> {
    let (deck, resources, first, imported) = io::load(&input.input, &input.font, page)?;
    Ok((
        Compiler::new(resources)?.compile_deck(deck)?,
        first,
        imported,
    ))
}
fn review(
    input: &Input,
    args: &ReviewArgs,
    d: &Deck,
    r: &Resources,
    planned: &import::Snapshot,
) -> Result<Option<baseline::Review>> {
    let default = io::source_root(&input.input).join(".slide-shuttle/base.json");
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
        Command::Pack {
            input,
            out,
            full_fonts,
        } => {
            if out
                .extension()
                .is_none_or(|e| !e.eq_ignore_ascii_case("qppt"))
            {
                return Err(fail("pack output must end with .qppt"));
            }
            if out.exists() {
                return Err(fail("QPPT output already exists; choose a new path"));
            }
            let mut project = io::project(&input.input)?;
            for font in &input.font {
                project.add_font(io::read(font, 64 * 1024 * 1024)?)?;
            }
            let deck = project.deck()?;
            let mut compiler = Compiler::new(project.resources(&deck)?)?;
            compiler.collect_font_usage(!full_fonts);
            let doc = compiler.compile_deck(deck)?;
            drop(compiler);
            if doc.scene().has_errors() {
                print(&doc.inspect())?;
                return Err(fail("layout has errors"));
            }
            let fonts = if full_fonts {
                json!({"mode":"full","embeddedBytes":doc.resources().fonts.iter().map(|f| f.bytes.len()).sum::<usize>()})
            } else {
                fonts::compact(&mut project, &doc)?
            };
            let bytes = project.write()?;
            io::write_new(&out, &bytes)?;
            print(
                &json!({"output":out,"format":"qppt","pages":doc.page_count(),"imported":project.manifest.imported,"bytes":bytes.len(),"fonts":fonts}),
            )?;
        }
        Command::Unpack { input, out } => {
            let project = package::Project::read(&io::read(&input, package::MAX_BYTES as u64)?)?;
            io::write_directory(&out, |stage| {
                for (name, bytes) in project
                    .files
                    .iter()
                    .map(|(n, s)| (n, s.as_bytes()))
                    .chain(project.assets.iter().map(|(n, b)| (n, b.as_slice())))
                {
                    io::write_new(&stage.join(name), bytes)?;
                }
                io::write_new(
                    &stage.join("manifest.json"),
                    &serde_json::to_vec_pretty(&project.manifest)?,
                )?;
                Ok(())
            })?;
            print(
                &json!({"output":out,"entry":out.join(package::ENTRY),"format":"qppt","imported":project.manifest.imported}),
            )?;
        }
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
            let (doc, _, _) = compile(&input, page.as_deref())?;
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
            steps,
        } => {
            let (doc, first, _) = compile(&input, page.as_deref())?;
            let (s, r) = (doc.scene(), doc.resources());
            io::write_directory(&out, |stage| {
                // Keep only one encoded page/state at a time; staging preserves validation
                // before publication without accumulating a long presentation in RAM.
                for (i, p) in s.pages.iter().enumerate() {
                    io::write_new(
                        &stage.join(format!("slide-{:03}.png", first + i + 1)),
                        &render::render_png(s, i, r, scale)?,
                    )?;
                    if steps && !p.show.animations.is_empty() {
                        for k in 0..show::step_count(&p.show) {
                            let looks = show::step(&p.show, k)?;
                            io::write_new(
                                &stage.join(format!("slide-{:03}-step-{k}.png", first + i + 1)),
                                &render::render_png_step(s, i, r, scale, &looks)?,
                            )?;
                        }
                    }
                }
                if pdf {
                    io::write_new(&stage.join("deck.pdf"), &doc.render_pdf()?)?;
                }
                io::write_new(
                    &stage.join("manifest.json"),
                    &serde_json::to_vec_pretty(
                        &json!({"engine":baseline::ENGINE,"resourceProfile":doc.resource_profile(),"pages":s.pages.len(),"width":s.width,"height":s.height,"diagnostics":s.diagnostics,
                        "slides":s.pages.iter().enumerate().map(|(i,p)|json!({"id":p.id,"file":format!("slide-{:03}.png",first+i+1),"stepCount":show::step_count(&p.show),"stepsExported":steps && !p.show.animations.is_empty(),"show":p.show})).collect::<Vec<_>>()}),
                    )?,
                )?;
                Ok(())
            })?;
            print(&json!({"output":out,"pages":s.pages.len(),"nativeRenderer":true}))?;
        }
        Command::Export {
            input,
            out,
            review: args,
            accept,
            report,
        } => {
            let (doc, _, imported) = compile(&input, None)?;
            if imported
                && args.base.is_none()
                && !io::source_root(&input.input)
                    .join(".slide-shuttle/base.json")
                    .exists()
            {
                return Err(fail(
                    "imported QPPT requires the original private baseline via --base and current loss decisions before PPTX rebuilding",
                ));
            }
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
            let dir = io::source_root(&input.input)
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
            if input
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("qppt"))
                || input.is_dir()
            {
                let project = io::project(&input)?;
                print(
                    &json!({"format":"qppt","manifest":project.manifest,"sources":project.files.keys().collect::<Vec<_>>(),"assets":project.assets.keys().collect::<Vec<_>>(),"pages":project.deck()?.slides.len()}),
                )?;
            } else {
                print(&inspect_pptx(&io::read(&input, 64 * 1024 * 1024)?)?)?;
            }
        }
        Command::Diff {
            input,
            review: args,
            out,
        } => {
            let (doc, _, _) = compile(&input, None)?;
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
