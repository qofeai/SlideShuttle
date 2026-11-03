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
}
#[wasm_bindgen]
pub struct CompiledDeck {
    document: Arc<CompiledDocument>,
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
        serde_json::to_string(&self.document.inspect()).map_err(js)
    }
    #[wasm_bindgen(js_name=renderPng)]
    pub fn render_png(&self, page: usize, scale: f32) -> std::result::Result<Vec<u8>, JsValue> {
        self.document.render_png(page, scale).map_err(js)
    }
    #[wasm_bindgen(js_name=renderPdf)]
    pub fn render_pdf(&self) -> std::result::Result<Vec<u8>, JsValue> {
        self.document.render_pdf().map_err(js)
    }
    #[wasm_bindgen(js_name=exportPptx)]
    pub fn export_pptx(&self) -> std::result::Result<Vec<u8>, JsValue> {
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
    pub fn add_font(&mut self, family: String, bytes: Vec<u8>) -> std::result::Result<(), JsValue> {
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
    pub fn compile(&mut self, source: &str) -> std::result::Result<CompiledDeck, JsValue> {
        Ok(CompiledDeck {
            document: self.document(source)?,
        })
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
    /// Source-based calls share the same single-entry compiled cache.
    pub fn check(&mut self, source: &str) -> std::result::Result<String, JsValue> {
        let d = self.document(source)?;
        serde_json::to_string(&serde_json::json!({"valid":!d.scene().has_errors(),"diagnostics":d.scene().diagnostics,"pages":d.page_count()})).map_err(js)
    }
    #[wasm_bindgen(js_name=renderPng)]
    pub fn render_png(
        &mut self,
        source: &str,
        page: usize,
        scale: f32,
    ) -> std::result::Result<Vec<u8>, JsValue> {
        self.document(source)?.render_png(page, scale).map_err(js)
    }
    #[wasm_bindgen(js_name=renderPdf)]
    pub fn render_pdf(&mut self, source: &str) -> std::result::Result<Vec<u8>, JsValue> {
        self.document(source)?.render_pdf().map_err(js)
    }
    #[wasm_bindgen(js_name=exportPptx)]
    pub fn export_pptx(&mut self, source: &str) -> std::result::Result<Vec<u8>, JsValue> {
        self.document(source)?.export_pptx().map_err(js)
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
            &serde_json::json!({"source":imported.xml,"snapshot":imported.snapshot}),
        )
        .map_err(js)?;
        self.invalidate();
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
    }
}
impl Engine {
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
    fn document(&mut self, source: &str) -> std::result::Result<Arc<CompiledDocument>, JsValue> {
        if let Some((cached, doc)) = &self.cached
            && cached == source
        {
            self.cache_hits += 1;
            return Ok(doc.clone());
        }
        self.cached = None;
        let document = Arc::new(self.compiler()?.compile(source).map_err(js)?);
        self.compilations += 1;
        self.cached = Some((source.into(), document.clone()));
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
