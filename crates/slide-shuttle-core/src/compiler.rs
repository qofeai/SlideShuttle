//! Reusable font system and immutable, resource-owning compiled documents.
use crate::{text::TextEngine, *};
use serde::Serialize;
use serde_json::json;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

pub struct Compiler {
    resources: Arc<Resources>,
    text: TextEngine,
    profile: String,
    compilations: u64,
    fallback_family: Option<String>,
}

#[derive(Clone)]
pub struct CompiledDocument {
    deck: Deck,
    scene: Scene,
    resources: Arc<Resources>,
    export_id: String,
    profile: String,
    png_cache: Arc<Mutex<PngCache>>,
}

#[derive(Default)]
struct PngCache {
    entries: VecDeque<((usize, u32), Vec<u8>)>,
    bytes: usize,
    renders: u64,
    hits: u64,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderStats {
    pub png_renders: u64,
    pub png_cache_hits: u64,
    pub cached_pages: usize,
    pub cached_bytes: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextMeasurement {
    pub width: f32,
    pub height: f32,
    pub line_count: usize,
    pub glyph_count: usize,
}

impl Compiler {
    pub fn new(resources: Resources) -> Result<Self> {
        Self::from_shared(Arc::new(resources))
    }
    pub fn from_shared(resources: Arc<Resources>) -> Result<Self> {
        let size = resources
            .fonts
            .iter()
            .map(|f| f.bytes.len())
            .chain(resources.images.values().map(Vec::len))
            .try_fold(0usize, |a, b| a.checked_add(b))
            .ok_or_else(|| model::invalid("resource size overflow"))?;
        if size > 256 * 1024 * 1024 {
            return Err(model::invalid("resource budget exceeds 256 MiB"));
        }
        let text = TextEngine::new(&resources)?;
        let profile = baseline::profile(&resources)?;
        Ok(Self {
            resources,
            text,
            profile,
            compilations: 0,
            fallback_family: None,
        })
    }
    pub fn compile(&mut self, source: &str) -> Result<CompiledDocument> {
        self.compile_deck(parse(source)?)
    }
    /// Compile a deck parsed elsewhere, e.g. a multi-file project from [`parse_project`].
    pub fn compile_deck(&mut self, mut deck: Deck) -> Result<CompiledDocument> {
        let substitutions = if let Some(family) = &self.fallback_family {
            crate::font_fallback::apply(&mut deck, family, &self.text)?
        } else {
            Vec::new()
        };
        fn bind(
            nodes: &mut [Node],
            assets: &std::collections::BTreeMap<String, String>,
            resources: &Resources,
        ) {
            for node in nodes {
                if let Kind::Image { src } = &mut node.kind
                    && !resources.images.contains_key(src)
                    && let Some(path) = assets.get(src)
                    && resources.images.contains_key(path)
                {
                    *src = path.clone();
                }
                bind(&mut node.children, assets, resources);
            }
        }
        for slide in &mut deck.slides {
            bind(&mut slide.nodes, &deck.assets, &self.resources);
            if let Some(src) = layout::background_image(&slide.background)
                && !self.resources.images.contains_key(src)
                && let Some(path) = deck.assets.get(src)
                && self.resources.images.contains_key(path)
            {
                slide.background = format!("url({path})");
            }
        }
        let mut scene = layout::layout_with_text(&deck, &self.resources, &mut self.text)?;
        scene.diagnostics.extend(substitutions);
        let export_id = digest(&serde_json::to_vec(&json!([
            baseline::ENGINE,
            deck,
            self.profile
        ]))?);
        self.compilations += 1;
        Ok(CompiledDocument {
            deck,
            scene,
            resources: self.resources.clone(),
            export_id,
            profile: self.profile.clone(),
            png_cache: Arc::new(Mutex::new(PngCache::default())),
        })
    }
    pub fn compilations(&self) -> u64 {
        self.compilations
    }
    /// Opt into reported font substitutions for subsequent compilations.
    /// None restores strict mode. Existing documents and measurements are unchanged.
    pub fn set_fallback_font(&mut self, family: Option<String>) -> Result<()> {
        if let Some(family) = &family {
            let style = TextStyle {
                family: family.clone(),
                ..TextStyle::default()
            };
            if !self.text.has_face(&style) {
                return Err(model::invalid(format!(
                    "fallback font regular face not supplied: {family}"
                )));
            }
        }
        self.fallback_family = family;
        Ok(())
    }
    pub fn measure_text(
        &mut self,
        text: &str,
        style: &TextStyle,
        width: Option<f32>,
        wrap: bool,
    ) -> Result<TextMeasurement> {
        if text.len() > 64 * 1024 {
            return Err(model::invalid("text measurement exceeds 64 KiB"));
        }
        if width.is_some_and(|w| !w.is_finite() || w <= 0.0 || w > 16384.0) {
            return Err(model::invalid("measurement width must be in (0,16384]px"));
        }
        if ![400, 700].contains(&style.weight) {
            return Err(model::invalid("invalid text measurement style"));
        }
        let shape = self
            .text
            .shape(&[Paragraph::plain(text)], style, width, wrap)?;
        Ok(TextMeasurement {
            width: shape.width,
            height: shape.height,
            line_count: shape.lines,
            glyph_count: shape.glyphs.len(),
        })
    }
}

impl CompiledDocument {
    pub fn deck(&self) -> &Deck {
        &self.deck
    }
    pub fn scene(&self) -> &Scene {
        &self.scene
    }
    pub fn resources(&self) -> &Resources {
        &self.resources
    }
    pub fn page_count(&self) -> usize {
        self.scene.pages.len()
    }
    pub fn resource_profile(&self) -> &str {
        &self.profile
    }
    pub fn render_png(&self, page: usize, scale: f32) -> Result<Vec<u8>> {
        const BUDGET: usize = 32 * 1024 * 1024;
        let key = (page, scale.to_bits());
        {
            let mut cache = self
                .png_cache
                .lock()
                .map_err(|_| model::invalid("render cache lock poisoned"))?;
            if let Some(index) = cache.entries.iter().position(|(k, _)| *k == key) {
                let entry = cache.entries.remove(index).unwrap();
                let result = entry.1.clone();
                cache.entries.push_back(entry);
                cache.hits += 1;
                return Ok(result);
            }
        }
        let bytes = render::render_png(&self.scene, page, &self.resources, scale)?;
        let mut cache = self
            .png_cache
            .lock()
            .map_err(|_| model::invalid("render cache lock poisoned"))?;
        cache.renders += 1;
        if bytes.len() <= BUDGET {
            // Another native rendering thread may have filled the same entry.
            if !cache.entries.iter().any(|(k, _)| *k == key) {
                while cache.bytes + bytes.len() > BUDGET || cache.entries.len() >= 8 {
                    if let Some((_, old)) = cache.entries.pop_front() {
                        cache.bytes -= old.len();
                    } else {
                        break;
                    }
                }
                cache.bytes += bytes.len();
                cache.entries.push_back((key, bytes.clone()));
            }
        }
        Ok(bytes)
    }
    pub fn render_stats(&self) -> Result<RenderStats> {
        let c = self
            .png_cache
            .lock()
            .map_err(|_| model::invalid("render cache lock poisoned"))?;
        Ok(RenderStats {
            png_renders: c.renders,
            png_cache_hits: c.hits,
            cached_pages: c.entries.len(),
            cached_bytes: c.bytes,
        })
    }
    pub fn render_pdf(&self) -> Result<Vec<u8>> {
        render::render_pdf(&self.scene, &self.resources)
    }
    pub fn export_pptx(&self) -> Result<Vec<u8>> {
        pptx::export_pptx(&self.scene, &self.resources, &self.export_id)
    }
    pub fn inspect(&self) -> serde_json::Value {
        json!({"valid":!self.scene.has_errors(),"width":self.scene.width,"height":self.scene.height,"pageCount":self.page_count(),"resourceProfile":self.profile,"diagnostics":self.scene.diagnostics,
            "pages":self.scene.pages.iter().map(|p|json!({"id":p.id,"nodes":p.items.iter().map(|i|{
                let text=match &i.kind{Draw::Text{line_count,glyphs,export_wrap,..}=>Some(json!({"lineCount":line_count,"glyphCount":glyphs.len(),"exportWrap":export_wrap})),_=>None};
                json!({"id":i.id,"rect":i.rect,"source":i.source,"text":text})
            }).collect::<Vec<_>>()})).collect::<Vec<_>>()})
    }
}
