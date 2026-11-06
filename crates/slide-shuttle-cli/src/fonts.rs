//! Portable, reshaping-capable subsets, built from the compiler's actual font use.
//! Kept in the CLI: viewing a QPPT must not ship or execute a font subsetter.
use hb_subset::{Blob, Flags, FontFace, SubsetInput};
use slide_shuttle_core::{
    package::{Project, path},
    *,
};
use std::collections::{BTreeMap, BTreeSet};

fn fail(message: impl std::fmt::Display) -> Error {
    Error::Invalid(format!(
        "font subsetting: {message}; use --full-fonts to retain original fonts"
    ))
}

type GlyphKey = (usize, u32, u16);
type GlyphMap = BTreeMap<GlyphKey, GlyphKey>;

pub fn compact(project: &mut Project, original: &CompiledDocument) -> Result<serde_json::Value> {
    let before: usize = original
        .resources()
        .fonts
        .iter()
        .map(|f| f.bytes.len())
        .sum();
    let mut usage = original.font_usage().clone();
    // Even a shapes-only deck needs a valid font database, but no full font.
    if usage.is_empty() {
        usage.insert((0, 0), FontUsage::default());
    }
    let mut subsets = Vec::new();
    for ((font_index, face_index), used) in &usage {
        let input_bytes = &original.resources().fonts[*font_index].bytes;
        let font =
            FontFace::new_with_index(Blob::from_bytes(input_bytes).map_err(fail)?, *face_index)
                .map_err(fail)?;
        let mut input = SubsetInput::new().map_err(fail)?;
        let drop_tables = input.drop_table_tag_set().clone_static();
        // keep_everything uses inverted sets, not a 2^32-entry allocation. Retain
        // all names/scripts/features, then restrict glyphs and restore safe drops.
        input.keep_everything();
        input.drop_table_tag_set().copy_from(&drop_tables);
        *input.flags() = Flags::default();
        input.flags().retain_notdef_outline();
        input.glyph_set().clear();
        input.unicode_set().clear();
        for glyph in &used.glyphs {
            input.glyph_set().insert(u32::from(*glyph));
        }
        for ch in used.unicodes.iter().filter_map(|c| char::from_u32(*c)) {
            input.unicode_set().insert(ch);
        }
        let plan = input.plan(&font).map_err(fail)?;
        let bytes = plan.subset().map_err(fail)?.underlying_blob().to_vec();
        let mapping = plan
            .old_to_new_glyph_mapping()
            .iter()
            .map(|(a, b)| (a as u16, b as u16))
            .collect::<Vec<_>>();
        verify_outlines(input_bytes, *face_index, &bytes, &mapping, used)?;
        subsets.push((*font_index, *face_index, bytes, mapping));
    }

    let old_fonts: BTreeSet<String> = original
        .deck()
        .fonts
        .iter()
        .map(|f| f.src.clone())
        .chain(project.manifest.fonts.iter().cloned())
        .map(|name| path(&name))
        .collect::<Result<_>>()?;
    let media: BTreeSet<String> = original
        .resources()
        .images
        .keys()
        .map(|name| path(name))
        .collect::<Result<_>>()?;
    // The manifest now owns the individual subset faces. Source font-family styles
    // remain intact; only file declarations change. Never rewrite the source on disk.
    for source in project.files.values_mut() {
        let xml = roxmltree::Document::parse(source)?;
        let ranges = xml
            .descendants()
            .filter(|n| n.has_tag_name("Font"))
            .map(|n| n.range())
            .collect::<Vec<_>>();
        for range in ranges.into_iter().rev() {
            source.replace_range(range, "");
        }
    }
    for name in old_fonts {
        if !media.contains(&name) {
            project.assets.remove(&name);
        }
    }
    project.manifest.fonts.clear();
    let mut glyph_map = GlyphMap::new();
    for (font_index, face_index, bytes, mapping) in subsets {
        let extension = if bytes.starts_with(b"OTTO") {
            "otf"
        } else {
            "ttf"
        };
        let name = format!("fonts/{}.{}", digest(&bytes), extension);
        project.add_font(bytes)?;
        let new_index = project
            .manifest
            .fonts
            .iter()
            .position(|n| n == &name)
            .unwrap();
        for (old, new) in mapping {
            glyph_map.insert((font_index, face_index, old), (new_index, 0, new));
        }
    }
    let deck = project.deck()?;
    let compact = Compiler::new(project.resources(&deck)?)?.compile_deck(deck)?;
    if compact.scene().has_errors() {
        return Err(fail("the compact document failed layout validation"));
    }
    // Re-shape, don't just verify the cmap. This catches altered fallback, ligatures,
    // bidi, line breaks, vertical forms and table/chart labels before publishing.
    if signature(original.scene(), Some(&glyph_map))? != signature(compact.scene(), None)? {
        return Err(fail("subsetting changed glyph selection or layout"));
    }
    let after: usize = compact
        .resources()
        .fonts
        .iter()
        .map(|f| f.bytes.len())
        .sum();
    Ok(
        serde_json::json!({"mode":"subset","originalBytes":before,"embeddedBytes":after,"faces":project.manifest.fonts.len()}),
    )
}

/// Validate outlines as well as metrics: a subset with a cmap but missing CFF/glyf
/// data can shape correctly while painting invisible text.
fn verify_outlines(
    original: &[u8],
    index: u32,
    subset: &[u8],
    mapping: &[(u16, u16)],
    used: &FontUsage,
) -> Result<()> {
    let before = ttf_parser::Face::parse(original, index).map_err(fail)?;
    let after = ttf_parser::Face::parse(subset, 0).map_err(fail)?;
    if before.units_per_em() != after.units_per_em() {
        return Err(fail("font units changed"));
    }
    for &(old, new) in mapping {
        if !used.glyphs.contains(&old) {
            continue;
        }
        let mut a = Outline::default();
        let mut b = Outline::default();
        let a_box = before.outline_glyph(ttf_parser::GlyphId(old), &mut a);
        let b_box = after.outline_glyph(ttf_parser::GlyphId(new), &mut b);
        if a_box != b_box || a.0 != b.0 {
            return Err(fail(format!("outline changed for glyph {old}")));
        }
    }
    Ok(())
}

#[derive(Default)]
struct Outline(Vec<(u8, Vec<f32>)>);
impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.push((0, vec![x, y]));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.push((1, vec![x, y]));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0.push((2, vec![x1, y1, x, y]));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.push((3, vec![x1, y1, x2, y2, x, y]));
    }
    fn close(&mut self) {
        self.0.push((4, vec![]));
    }
}

fn signature(scene: &Scene, map: Option<&GlyphMap>) -> Result<Vec<serde_json::Value>> {
    fn visit(item: &Item, map: Option<&GlyphMap>, out: &mut Vec<serde_json::Value>) -> Result<()> {
        out.push(serde_json::json!([item.id, item.rect]));
        match &item.kind {
            Draw::Text {
                glyphs, line_count, ..
            } => {
                out.push(serde_json::json!(line_count));
                for g in glyphs {
                    let key = (g.font, g.font_index, g.glyph_id);
                    let key = match map {
                        Some(map) => *map.get(&key).ok_or_else(|| fail("missing shaped glyph"))?,
                        None => key,
                    };
                    out.push(serde_json::json!([
                        key, g.text, g.x, g.y, g.advance, g.size, g.weight, g.color, g.sideways
                    ]));
                }
            }
            Draw::Table { cells, .. } => {
                for item in cells {
                    visit(item, map, out)?;
                }
            }
            Draw::Chart { primitives, .. } => {
                for item in primitives {
                    visit(item, map, out)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    let mut result = Vec::new();
    for page in &scene.pages {
        result.push(serde_json::json!(page.id));
        visit(&page.background, map, &mut result)?;
        for item in &page.items {
            visit(item, map, &mut result)?;
        }
    }
    Ok(result)
}
