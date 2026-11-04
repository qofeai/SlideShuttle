//! Compile once, render many times. Documents own immutable resource snapshots.
use slide_shuttle_core::*;
use std::sync::Arc;
use wasm_bindgen::prelude::*;
fn js(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

#[wasm_bindgen]
#[derive(Default)]
pub struct Engine {
    resources: Arc<Resources>,
    compiler: Option<Compiler>,
    cached: Option<(String, Arc<CompiledDocument>)>,
    compilations: u32,
    font_builds: u32,
    cache_hits: u32,
    fallback_family: Option<String>,
    imported: bool,
    losses: Vec<import::Loss>,
}
#[wasm_bindgen]
pub struct CompiledDeck {
    document: Arc<CompiledDocument>,
    imported: bool,
    losses: Vec<import::Loss>,
}

#[wasm_bindgen]
impl CompiledDeck {
    pub fn stats(&self) -> std::result::Result<String, JsValue> {
        serde_json::to_string(&self.document.render_stats().map_err(js)?).map_err(js)
    }
    #[wasm_bindgen(getter,js_name=pageCount)]
    pub fn page_count(&self) -> usize {
        self.document.page_count()
    }
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> f32 {
        self.document.scene().width
    }
    #[wasm_bindgen(getter)]
    pub fn height(&self) -> f32 {
        self.document.scene().height
    }
    pub fn inspect(&self) -> std::result::Result<String, JsValue> {
        let mut report = self.document.inspect();
        report["imported"] = serde_json::json!(self.imported);
        report["losses"] = serde_json::json!(self.losses);
        serde_json::to_string(&report).map_err(js)
    }
    #[wasm_bindgen(js_name=renderPng)]
    pub fn render_png(&self, page: usize, scale: f32) -> std::result::Result<Vec<u8>, JsValue> {
        self.document.render_png(page, scale).map_err(js)
    }
    #[wasm_bindgen(js_name=stepCount)]
    pub fn step_count(&self, page: usize) -> std::result::Result<usize, JsValue> {
        self.document.step_count(page).map_err(js)
    }
    #[wasm_bindgen(js_name=renderPngStep)]
    pub fn render_png_step(
        &self,
        page: usize,
        step: usize,
        scale: f32,
    ) -> std::result::Result<Vec<u8>, JsValue> {
        self.document.render_png_step(page, step, scale).map_err(js)
    }
    /// Straight-alpha RGBA8; dimensions are ceil(width * scale), ceil(height * scale).
    #[wasm_bindgen(js_name=renderRgba)]
    pub fn render_rgba(&self, page: usize, scale: f32) -> std::result::Result<Vec<u8>, JsValue> {
        Ok(self.document.render_rgba(page, scale).map_err(js)?.pixels)
    }
    #[wasm_bindgen(js_name=renderRgbaStep)]
    pub fn render_rgba_step(
        &self,
        page: usize,
        step: usize,
        scale: f32,
    ) -> std::result::Result<Vec<u8>, JsValue> {
        Ok(self
            .document
            .render_rgba_step(page, step, scale)
            .map_err(js)?
            .pixels)
    }
    #[wasm_bindgen(js_name=renderPdf)]
    pub fn render_pdf(&self) -> std::result::Result<Vec<u8>, JsValue> {
        self.document.render_pdf().map_err(js)
    }
    #[wasm_bindgen(js_name=exportPptx)]
    pub fn export_pptx(&self) -> std::result::Result<Vec<u8>, JsValue> {
        if self.imported {
            return Err(js(
                "imported document PPTX rebuilding requires CLI baseline and loss decisions",
            ));
        }
        self.document.export_pptx().map_err(js)
    }
}

#[wasm_bindgen]
impl Engine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }
    #[wasm_bindgen(js_name=addFont)]
    pub fn add_font(
        &mut self,
        family: Option<String>,
        bytes: Vec<u8>,
    ) -> std::result::Result<(), JsValue> {
        self.budget(bytes.len(), 0)?;
        if bytes.len() > 64 * 1024 * 1024 {
            return Err(js("font exceeds 64 MiB"));
        }
        self.invalidate();
        Arc::make_mut(&mut self.resources)
            .fonts
            .push(FontResource { family, bytes });
        Ok(())
    }
    #[wasm_bindgen(js_name=addImage)]
    pub fn add_image(&mut self, name: String, bytes: Vec<u8>) -> std::result::Result<(), JsValue> {
        if name.is_empty() || name.len() > 256 || bytes.len() > 32 * 1024 * 1024 {
            return Err(js("invalid image key or excessive image size"));
        }
        let replaced = self.resources.images.get(&name).map_or(0, Vec::len);
        self.budget(bytes.len(), replaced)?;
        self.invalidate();
        Arc::make_mut(&mut self.resources)
            .images
            .insert(name, bytes);
        Ok(())
    }
    /// Compile a multi-file project. `files_json` is a JSON object from project-relative
    /// path to source text; `entry` names the `<Deck>` file.
    #[wasm_bindgen(js_name=compileProject)]
    pub fn compile_project(
        &mut self,
        entry: &str,
        files_json: &str,
    ) -> std::result::Result<CompiledDeck, JsValue> {
        if files_json.len() > 64 * 1024 * 1024 {
            return Err(js("project sources exceed 64 MiB"));
        }
        let key = format!("{entry}\0{files_json}");
        let document = self.cached_or(key, |compiler| {
            let files: std::collections::BTreeMap<String, String> =
                serde_json::from_str(files_json)?;
            package::require_project(
                files
                    .get(entry)
                    .ok_or_else(|| Error::Invalid("missing project entry".into()))?,
            )?;
            compiler.compile_deck(parse_files(entry, &files)?)
        })?;
        Ok(CompiledDeck {
            document,
            imported: self.imported,
            losses: self.losses.clone(),
        })
    }
    /// Open a self-contained QPPT directly, replacing the Engine's active resources.
    #[wasm_bindgen(js_name=openQppt)]
    pub fn open_qppt(&mut self, bytes: &[u8]) -> std::result::Result<CompiledDeck, JsValue> {
        let project = self.load_qppt(bytes)?;
        self.compile_project(
            package::ENTRY,
            &serde_json::to_string(&project.files).map_err(js)?,
        )
    }
    /// Load QPPT resources and return editable source files and public import metadata.
    #[wasm_bindgen(js_name=importQppt)]
    pub fn import_qppt(&mut self, bytes: &[u8]) -> std::result::Result<String, JsValue> {
        let project = self.load_qppt(bytes)?;
        serde_json::to_string(&serde_json::json!({"entry":package::ENTRY,"files":project.files,"manifest":project.manifest})).map_err(js)
    }
    /// Optional, reported preview substitutions. The default remains strict.
    #[wasm_bindgen(js_name=setFontFallback)]
    pub fn set_font_fallback(
        &mut self,
        family: Option<String>,
    ) -> std::result::Result<(), JsValue> {
        self.compiler()?
            .set_fallback_font(family.clone())
            .map_err(js)?;
        self.fallback_family = family;
        self.cached = None;
        Ok(())
    }
    #[wasm_bindgen(js_name=measureText)]
    pub fn measure_text(
        &mut self,
        text: &str,
        style_json: &str,
        width: Option<f32>,
        wrap: bool,
    ) -> std::result::Result<String, JsValue> {
        if style_json.len() > 4096 {
            return Err(js("measurement style exceeds budget"));
        }
        let style: TextStyle = serde_json::from_str(style_json).map_err(js)?;
        let measured = self
            .compiler()?
            .measure_text(text, &style, width, wrap)
            .map_err(js)?;
        serde_json::to_string(&measured).map_err(js)
    }
    #[wasm_bindgen(js_name=importPptx)]
    pub fn import_pptx(&mut self, bytes: &[u8]) -> std::result::Result<String, JsValue> {
        let imported = import::import_pptx(bytes).map_err(js)?;
        let added = imported.assets.values().map(Vec::len).sum::<usize>();
        let replaced = imported
            .assets
            .keys()
            .filter_map(|key| self.resources.images.get(key))
            .map(Vec::len)
            .sum::<usize>();
        self.budget(added, replaced)?;
        let result = serde_json::to_string(
            &serde_json::json!({"entry":"deck.xml","files":imported.files,"snapshot":imported.snapshot}),
        )
        .map_err(js)?;
        self.invalidate();
        self.imported = true;
        self.losses = imported.snapshot.losses.clone();
        Arc::make_mut(&mut self.resources)
            .images
            .extend(imported.assets);
        Ok(result)
    }
    pub fn stats(&self) -> String {
        serde_json::json!({"compilations":self.compilations,"fontBuilds":self.font_builds,"cacheHits":self.cache_hits}).to_string()
    }
    pub fn clear(&mut self) {
        self.invalidate();
        self.resources = Arc::new(Resources::default());
        self.fallback_family = None;
        self.imported = false;
        self.losses.clear();
    }
}
impl Engine {
    fn load_qppt(&mut self, bytes: &[u8]) -> std::result::Result<package::Project, JsValue> {
        let mut project = package::Project::read(bytes).map_err(js)?;
        let resources = project
            .take_resources(&project.deck().map_err(js)?)
            .map_err(js)?;
        self.invalidate();
        self.resources = Arc::new(resources);
        self.fallback_family = None;
        self.imported = project.manifest.imported;
        self.losses = project.manifest.losses.clone();
        Ok(project)
    }
    fn invalidate(&mut self) {
        self.cached = None;
        self.compiler = None;
    }
    fn compiler(&mut self) -> std::result::Result<&mut Compiler, JsValue> {
        if self.compiler.is_none() {
            let mut compiler = Compiler::from_shared(self.resources.clone()).map_err(js)?;
            compiler
                .set_fallback_font(self.fallback_family.clone())
                .map_err(js)?;
            self.compiler = Some(compiler);
            self.font_builds += 1;
        }
        Ok(self.compiler.as_mut().unwrap())
    }
    /// One cached compilation, keyed by everything that was compiled.
    fn cached_or(
        &mut self,
        key: String,
        compile: impl FnOnce(&mut Compiler) -> Result<CompiledDocument>,
    ) -> std::result::Result<Arc<CompiledDocument>, JsValue> {
        if let Some((cached, doc)) = &self.cached
            && *cached == key
        {
            self.cache_hits += 1;
            return Ok(doc.clone());
        }
        self.cached = None;
        let document = Arc::new(compile(self.compiler()?).map_err(js)?);
        self.compilations += 1;
        self.cached = Some((key, document.clone()));
        Ok(document)
    }
    fn budget(&self, extra: usize, replaced: usize) -> std::result::Result<(), JsValue> {
        let used = self
            .resources
            .fonts
            .iter()
            .map(|f| f.bytes.len())
            .sum::<usize>()
            + self.resources.images.values().map(Vec::len).sum::<usize>();
        if extra > 256 * 1024 * 1024 || used.saturating_sub(replaced) > 256 * 1024 * 1024 - extra {
            Err(js("resource budget exceeds 256 MiB"))
        } else {
            Ok(())
        }
    }
}
