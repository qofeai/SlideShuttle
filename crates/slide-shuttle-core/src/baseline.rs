//! Conservative three-way review. This module does not merge or patch Office files.
use crate::{
    import::{Object, Snapshot},
    model::*,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const ENGINE: &str = concat!(
    "slide-shuttle-core/",
    env!("CARGO_PKG_VERSION"),
    "/normalizer-1"
);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Baseline {
    pub version: u32,
    pub base_id: String,
    pub engine: String,
    pub resources: String,
    pub source: Deck,
    pub native: Snapshot,
    pub imported: bool,
}
impl Baseline {
    pub fn new(source: Deck, native: Snapshot, resources: String) -> Result<Self> {
        let base_id = digest(&serde_json::to_vec(&json!([
            ENGINE,
            resources,
            source,
            native.digest
        ]))?);
        Ok(Self {
            version: 1,
            base_id,
            engine: ENGINE.into(),
            resources,
            source,
            native,
            imported: false,
        })
    }
    pub fn from_import(source: Deck, native: Snapshot, resources: String) -> Result<Self> {
        let mut base = Self::new(source, native, resources)?;
        base.imported = true;
        base.base_id = digest(format!("import:{}", base.base_id).as_bytes());
        Ok(base)
    }
}

pub fn resource_signature(resources: &Resources) -> Result<String> {
    Ok(signatures(resources)?.1)
}
/// (font signature, resource signature), hashing each resource byte once.
fn signatures(resources: &Resources) -> Result<(String, String)> {
    let fonts = resources
        .fonts
        .iter()
        .map(|f| (&f.family, digest(&f.bytes)))
        .collect::<Vec<_>>();
    let images = resources
        .images
        .iter()
        .map(|(n, b)| (n, digest(b)))
        .collect::<BTreeMap<_, _>>();
    Ok((
        digest(&serde_json::to_vec(&fonts)?),
        digest(&serde_json::to_vec(&json!([fonts, images]))?),
    ))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub id: String,
    pub object: String,
    pub property: String,
    pub status: String,
    pub blocking: bool,
    pub base: Value,
    pub external: Value,
    pub planned: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub version: u32,
    pub diff_id: String,
    pub base_id: String,
    pub source_digest: String,
    pub external_digest: Option<String>,
    pub planned_digest: String,
    pub external_checked: bool,
    pub changes: Vec<Change>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Acceptance {
    pub diff_id: String,
    pub items: BTreeMap<String, String>,
}
impl Review {
    pub fn unresolved(&self, acceptance: Option<&Acceptance>) -> Result<Vec<&Change>> {
        if let Some(a) = acceptance {
            if a.diff_id != self.diff_id {
                return Err(invalid(
                    "acceptance is stale: diffId does not match current inputs",
                ));
            }
            for (id, reason) in &a.items {
                if reason.trim().is_empty()
                    || !self.changes.iter().any(|c| &c.id == id && c.blocking)
                {
                    return Err(invalid(
                        "acceptance must name current blocking change IDs and give a nonempty reason",
                    ));
                }
            }
        }
        Ok(self
            .changes
            .iter()
            .filter(|c| c.blocking && acceptance.is_none_or(|a| !a.items.contains_key(&c.id)))
            .collect())
    }
}

pub fn review(
    base: &Baseline,
    source: &Deck,
    resources: &Resources,
    external: Option<&Snapshot>,
    planned: &Snapshot,
) -> Result<Review> {
    let mut changes = Vec::new();
    let mut add = |object: &str,
                   property: &str,
                   status: &str,
                   blocking: bool,
                   b: Value,
                   e: Value,
                   p: Value| {
        let id = digest(&serde_json::to_vec(&json!([object, property, status, b, e, p])).unwrap());
        changes.push(Change {
            id,
            object: object.into(),
            property: property.into(),
            status: status.into(),
            blocking,
            base: b,
            external: e,
            planned: p,
        });
    };
    // Images can intentionally change. Only font identity and engine changes invalidate layout inference.
    let old_fonts = base.resources.split(':').next().unwrap_or("");
    let (current_fonts, resource_key) = signatures(resources)?;
    if base.version != 1 || base.engine != ENGINE || old_fonts != current_fonts {
        add(
            "engine",
            "profile",
            "engine_drift",
            true,
            json!([base.engine, old_fonts]),
            Value::Null,
            json!([ENGINE, current_fonts]),
        );
    }
    let e = external.unwrap_or(&base.native);
    if let Some(external) = external
        && base.native.export_id.is_some()
        && external.export_id != base.native.export_id
    {
        add(
            "package",
            "identity",
            "unknown",
            true,
            json!(base.native.export_id),
            json!(external.export_id),
            json!(planned.export_id),
        );
    }
    let old_nodes = nodes(&base.source);
    let new_nodes = nodes(source);
    fn groups(snapshot: &Snapshot) -> BTreeMap<String, Vec<&Object>> {
        let mut map = BTreeMap::<String, Vec<&Object>>::new();
        for o in &snapshot.objects {
            map.entry(format!("{}:{}", o.page, o.name))
                .or_default()
                .push(o);
        }
        map
    }
    let bmap = groups(&base.native);
    let emap = groups(e);
    let pmap = groups(planned);
    let keys = bmap
        .keys()
        .chain(emap.keys())
        .chain(pmap.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for key in keys {
        let b = bmap.get(&key);
        let e = emap.get(&key);
        let p = pmap.get(&key);
        if [b, e, p]
            .into_iter()
            .flatten()
            .any(|v| v.len() != 1 || v[0].name.is_empty())
        {
            add(
                &key,
                "identity",
                "ambiguous",
                true,
                json!(b),
                json!(e),
                json!(p),
            );
            continue;
        }
        let b = b.map(|v| v[0]);
        let e = e.map(|v| v[0]);
        let p = p.map(|v| v[0]);
        if b.is_none() || e.is_none() || p.is_none() {
            // A source deletion of an externally edited object is a conflict, even when presence matches B/E.
            let authored_removal = b.is_some_and(|o| {
                let id = o.name.strip_prefix("d:").unwrap_or(&o.name);
                let id = id.strip_suffix("::background").unwrap_or(id);
                old_nodes
                    .get(id)
                    .is_some_and(|old| new_nodes.get(id) != Some(old))
            });
            let status = if b == e && p.is_none() && authored_removal {
                "source_change"
            } else if b == e && p.is_none() && b.is_some() {
                "omission"
            } else {
                classify(&json!(b), &json!(e), &json!(p))
            };
            if status != "clean" {
                add(
                    &key,
                    "presence",
                    status,
                    is_blocking(status),
                    json!(b),
                    json!(e),
                    json!(p),
                );
            }
            continue;
        }
        let (b, e, p) = (b.unwrap(), e.unwrap(), p.unwrap());
        for (prop, bv, ev, pv) in [
            ("text", json!(b.text), json!(e.text), json!(p.text)),
            (
                "geometry",
                json!(b.geometry),
                json!(e.geometry),
                json!(p.geometry),
            ),
            ("style", b.style.clone(), e.style.clone(), p.style.clone()),
            (
                "payload",
                json!(b.payload),
                json!(e.payload),
                json!(p.payload),
            ),
            ("parent", json!(b.parent), json!(e.parent), json!(p.parent)),
            ("kind", json!(b.kind), json!(e.kind), json!(p.kind)),
            (
                "nativeDetail",
                json!(b.native_digest),
                json!(e.native_digest),
                json!(p.native_digest),
            ),
        ] {
            let mut status = classify(&bv, &ev, &pv);
            if status == "source_change" && prop == "geometry" {
                let id = b.name.strip_prefix("d:").unwrap_or(&b.name);
                if old_nodes.get(id) == new_nodes.get(id) {
                    status = "derived_change";
                }
            }
            if status != "clean" {
                add(&key, prop, status, is_blocking(status), bv, ev, pv);
            }
        }
    }
    for (property, b, e, p) in [
        (
            "pages",
            json!(base.native.pages),
            json!(e.pages),
            json!(planned.pages),
        ),
        (
            "pageSize",
            json!([base.native.width, base.native.height]),
            json!([e.width, e.height]),
            json!([planned.width, planned.height]),
        ),
    ] {
        let status = classify(&b, &e, &p);
        if status != "clean" {
            add("package", property, status, is_blocking(status), b, e, p);
        }
    }
    let parts = base
        .native
        .inventory
        .keys()
        .chain(e.inventory.keys())
        .chain(planned.inventory.keys())
        .collect::<BTreeSet<_>>();
    for name in parts {
        let b = json!(base.native.inventory.get(name));
        let ev = json!(e.inventory.get(name));
        let p = json!(planned.inventory.get(name));
        if b == ev && ev == p {
            continue;
        }
        // Loss of an existing part is never a harmless derived layout change.
        let blocking = (base.native.inventory.contains_key(name) || e.inventory.contains_key(name))
            && (p.is_null() || ev != b && p != ev);
        add(
            name,
            "packagePart",
            if blocking {
                "package_changed"
            } else {
                "source_change"
            },
            blocking,
            b,
            ev,
            p,
        );
    }
    // Import reconstruction losses apply to imported projects. Inspecting an authored
    // native Group must not make an unchanged authored deck fail its own review.
    let imported_losses = base.native.losses.iter().filter(|_| base.imported);
    let external_losses = e
        .losses
        .iter()
        .filter(|l| !base.native.losses.contains(l))
        .filter(|loss| {
            !e.objects
                .iter()
                .filter(|o| o.page == loss.page && o.name == loss.object)
                .any(|o| {
                    planned.objects.iter().any(|p| {
                        p.page == o.page && p.name == o.name && p.native_digest == o.native_digest
                    })
                })
        });
    for loss in imported_losses.chain(external_losses) {
        add(
            &format!("{}:{}", loss.page, loss.object),
            "importLoss",
            "unmodeled",
            true,
            json!(loss.reason),
            Value::Null,
            Value::Null,
        );
    }
    changes.sort_by(|a, b| a.id.cmp(&b.id));
    changes.dedup_by(|a, b| a.id == b.id);
    let source_digest = digest(&serde_json::to_vec(source)?);
    let external_digest = external.map(|e| e.digest.clone());
    let diff_id = digest(&serde_json::to_vec(&json!([
        base.base_id,
        source_digest,
        external_digest,
        planned.digest,
        resource_key,
        changes
    ]))?);
    Ok(Review {
        version: 1,
        diff_id,
        base_id: base.base_id.clone(),
        source_digest,
        external_digest,
        planned_digest: planned.digest.clone(),
        external_checked: external.is_some(),
        changes,
    })
}

pub fn profile(resources: &Resources) -> Result<String> {
    let (fonts, all) = signatures(resources)?;
    Ok(format!("{fonts}:{all}"))
}
fn classify(b: &Value, e: &Value, p: &Value) -> &'static str {
    if b == e && e == p {
        "clean"
    } else if e == b {
        "source_change"
    } else if e == p {
        "adopted"
    } else if p == b {
        "external_unresolved"
    } else {
        "conflict"
    }
}
fn is_blocking(s: &str) -> bool {
    !matches!(s, "clean" | "source_change" | "derived_change" | "adopted")
}
fn nodes(deck: &Deck) -> BTreeMap<String, Value> {
    fn visit(n: &Node, out: &mut BTreeMap<String, Value>) {
        out.insert(n.id.clone(), json!([n.style, n.kind]));
        for c in &n.children {
            visit(c, out);
        }
    }
    let mut out = BTreeMap::new();
    for s in &deck.slides {
        for n in &s.nodes {
            visit(n, &mut out);
        }
    }
    out
}
