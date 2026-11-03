//! Explicit preview policy. Resolve typed styles before layout so preview and
//! export use the same actual faces, while the caller's XML remains untouched.
use crate::{
    model::*,
    text::{TextEngine, text_style},
};
use std::collections::BTreeSet;

fn resolve(requested: &TextStyle, fallback: &str, text: &TextEngine) -> Result<TextStyle> {
    // Later families in a `font-family` list stay only when their face is supplied.
    let requested = &TextStyle {
        fallback: requested
            .fallback
            .iter()
            .filter(|f| {
                text.has_face(&TextStyle {
                    family: (*f).clone(),
                    ..requested.clone()
                })
            })
            .cloned()
            .collect(),
        ..requested.clone()
    };
    if text.has_face(requested) {
        return Ok(requested.clone());
    }
    // Preserve the family and weight first, then fall back without synthesizing
    // slant or bold. Every change is reported by the caller.
    let candidates = [
        (requested.family.as_str(), requested.weight, false),
        (fallback, requested.weight, requested.italic),
        (fallback, requested.weight, false),
        (fallback, 400, false),
    ];
    for (family, weight, italic) in candidates {
        let next = TextStyle {
            family: family.into(),
            weight,
            italic,
            ..requested.clone()
        };
        if text.has_face(&next) {
            return Ok(next);
        }
    }
    Err(invalid(format!(
        "fallback font face not supplied: {fallback}"
    )))
}

fn bind(
    css: &mut Css,
    requested: &TextStyle,
    resolved: &TextStyle,
    changes: &mut BTreeSet<String>,
) {
    // Explicit leaf styles prevent the substituted base from changing an
    // independently styled Span or a nested container's inheritance.
    css.insert(
        "font-family".into(),
        std::iter::once(&resolved.family)
            .chain(&resolved.fallback)
            .cloned()
            .collect::<Vec<_>>()
            .join(", "),
    );
    for dropped in requested
        .fallback
        .iter()
        .filter(|f| !resolved.fallback.contains(f))
    {
        changes.insert(format!(
            "font-family entry {dropped} has no supplied face; dropped"
        ));
    }
    css.insert("font-weight".into(), resolved.weight.to_string());
    css.insert(
        "font-style".into(),
        if resolved.italic { "italic" } else { "normal" }.into(),
    );
    if requested.family != resolved.family
        || requested.weight != resolved.weight
        || requested.italic != resolved.italic
    {
        changes.insert(format!(
            "font substituted: {} weight={} italic={} → {} weight={} italic={}",
            requested.family,
            requested.weight,
            requested.italic,
            resolved.family,
            resolved.weight,
            resolved.italic
        ));
    }
}

pub(crate) fn apply(deck: &mut Deck, fallback: &str, text: &TextEngine) -> Result<Vec<Diagnostic>> {
    fn walk(
        nodes: &mut [Node],
        parent: &TextStyle,
        page: &str,
        fallback: &str,
        text: &TextEngine,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<()> {
        for node in nodes {
            let requested = text_style(&node.style, parent)?;
            let mut changes = BTreeSet::new();
            if matches!(
                node.kind,
                Kind::Text { .. } | Kind::Table { .. } | Kind::Chart { .. }
            ) {
                let resolved = resolve(&requested, fallback, text)?;
                bind(&mut node.style, &requested, &resolved, &mut changes);
                // Each level is resolved against what its parent requested.
                let mut nested = |css: &mut Css, parent: &TextStyle| -> Result<TextStyle> {
                    let requested = text_style(css, parent)?;
                    let resolved = resolve(&requested, fallback, text)?;
                    bind(css, &requested, &resolved, &mut changes);
                    Ok(requested)
                };
                match &mut node.kind {
                    Kind::Text { paragraphs } => {
                        for span in paragraphs.iter_mut().flat_map(|p| &mut p.spans) {
                            nested(&mut span.style, &requested)?;
                        }
                    }
                    Kind::Table { rows, .. } => {
                        for row in rows {
                            let in_row = nested(&mut row.style, &requested)?;
                            for cell in &mut row.cells {
                                let in_cell = nested(&mut cell.style, &in_row)?;
                                for span in cell.paragraphs.iter_mut().flat_map(|p| &mut p.spans) {
                                    nested(&mut span.style, &in_cell)?;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            for message in changes {
                diagnostics.push(Diagnostic {
                    code: "font_substitution".into(),
                    severity: Severity::Warning,
                    page: page.into(),
                    node_id: node.id.clone(),
                    message,
                    source: Some(node.source.clone()),
                });
            }
            walk(
                &mut node.children,
                &requested,
                page,
                fallback,
                text,
                diagnostics,
            )?;
        }
        Ok(())
    }
    let parent = TextStyle {
        family: deck.font_family.clone(),
        ..TextStyle::default()
    };
    let mut diagnostics = Vec::new();
    for slide in &mut deck.slides {
        walk(
            &mut slide.nodes,
            &parent,
            &slide.id,
            fallback,
            text,
            &mut diagnostics,
        )?;
    }
    Ok(diagnostics)
}
