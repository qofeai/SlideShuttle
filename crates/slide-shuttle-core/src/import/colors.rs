//! DrawingML color values and ordered transforms. Unknown values are reported,
//! never silently treated as a successfully resolved theme color.
use super::{child, parse_xml};
use crate::{model::*, pptx::A};
use roxmltree::{Document, Node as X};
use std::collections::{BTreeMap, BTreeSet};

pub(super) type Problems = BTreeSet<String>;
pub(super) struct Theme<'i> {
    docs: Vec<Document<'i>>,
    pub map: BTreeMap<String, String>,
}

impl<'i> Theme<'i> {
    pub fn new(entries: &'i BTreeMap<String, Vec<u8>>, paths: &[String]) -> Result<Self> {
        let docs = paths
            .iter()
            .map(|p| {
                let bytes = entries
                    .get(p)
                    .ok_or_else(|| invalid(format!("missing theme part: {p}")))?;
                parse_xml(bytes)
            })
            .collect::<Result<Vec<_>>>()?;
        let map = [
            ("bg1", "lt1"),
            ("bg2", "lt2"),
            ("tx1", "dk1"),
            ("tx2", "dk2"),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();
        Ok(Self { docs, map })
    }
    pub fn element(&self, name: &str) -> Option<X<'_, 'i>> {
        self.docs.iter().rev().find_map(|doc| {
            let root = doc.root_element();
            child(root, A, "themeElements")
                .and_then(|n| child(n, A, name))
                .or_else(|| child(root, A, name))
        })
    }
    pub fn font(&self, requested: &str, cjk: bool, problems: &mut Problems) -> Option<String> {
        if !requested.starts_with('+') {
            return Some(requested.into());
        }
        let (major, script) = match requested {
            "+mj-lt" => ("majorFont", "latin"),
            "+mn-lt" => ("minorFont", "latin"),
            "+mj-ea" => ("majorFont", "ea"),
            "+mn-ea" => ("minorFont", "ea"),
            "+mj-cs" => ("majorFont", "cs"),
            "+mn-cs" => ("minorFont", "cs"),
            _ => {
                problems.insert(format!("unknown theme font reference: {requested}"));
                return None;
            }
        };
        let font = self.element("fontScheme").and_then(|n| child(n, A, major));
        let direct = font
            .and_then(|n| child(n, A, script))
            .and_then(|n| n.attribute("typeface"))
            .filter(|s| !s.is_empty());
        let supplemental = font
            .and_then(|n| {
                n.children().find(|f| {
                    f.has_tag_name((A, "font"))
                        && f.attribute("script") == Some(if cjk { "Hans" } else { "Latn" })
                })
            })
            .and_then(|n| n.attribute("typeface"))
            .filter(|s| !s.is_empty());
        let latin = font
            .and_then(|n| child(n, A, "latin"))
            .and_then(|n| n.attribute("typeface"))
            .filter(|s| !s.is_empty());
        match direct.or(supplemental).or(latin) {
            Some(name) => Some(name.into()),
            None => {
                problems.insert(format!("theme font cannot be resolved: {requested}"));
                None
            }
        }
    }
    pub fn color(
        &self,
        n: X<'_, '_>,
        placeholder: Option<Color>,
        problems: &mut Problems,
    ) -> Option<Color> {
        let Some(rgba) = self.resolve(n, placeholder.map(channels), 0, problems) else {
            problems.insert(format!(
                "color cannot be resolved: {} {}",
                n.tag_name().name(),
                n.attribute("val").unwrap_or("")
            ));
            return None;
        };
        let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Some(Color(
            byte(rgba[0]),
            byte(rgba[1]),
            byte(rgba[2]),
            byte(rgba[3]),
        ))
    }
    pub fn color_child(
        &self,
        n: X<'_, '_>,
        placeholder: Option<Color>,
        problems: &mut Problems,
    ) -> Option<Color> {
        let Some(color) = n.children().find(|x| {
            x.tag_name().namespace() == Some(A)
                && [
                    "srgbClr",
                    "schemeClr",
                    "sysClr",
                    "scrgbClr",
                    "hslClr",
                    "prstClr",
                ]
                .contains(&x.tag_name().name())
        }) else {
            problems.insert(format!("missing color value in {}", n.tag_name().name()));
            return None;
        };
        self.color(color, placeholder, problems)
    }
    pub fn fill(
        &self,
        n: X<'_, '_>,
        placeholder: Option<Color>,
        problems: &mut Problems,
    ) -> Option<Color> {
        let fill = if [
            "solidFill",
            "noFill",
            "gradFill",
            "pattFill",
            "blipFill",
            "grpFill",
        ]
        .contains(&n.tag_name().name())
        {
            Some(n)
        } else {
            n.children().find(|x| {
                x.tag_name().namespace() == Some(A)
                    && [
                        "solidFill",
                        "noFill",
                        "gradFill",
                        "pattFill",
                        "blipFill",
                        "grpFill",
                    ]
                    .contains(&x.tag_name().name())
            })
        }?;
        match fill.tag_name().name() {
            "noFill" => Some(Color(0, 0, 0, 0)),
            "solidFill" => self.color_child(fill, placeholder, problems),
            other => {
                problems.insert(format!("{other} fill is not reconstructed"));
                None
            }
        }
    }
    fn resolve(
        &self,
        n: X<'_, '_>,
        placeholder: Option<[f64; 4]>,
        depth: u8,
        problems: &mut Problems,
    ) -> Option<[f64; 4]> {
        if depth > 8 {
            problems.insert("cyclic theme color reference".into());
            return None;
        }
        let val = n.attribute("val").unwrap_or("");
        let mut rgba = match n.tag_name().name() {
            "srgbClr" => hex(val),
            "sysClr" => n.attribute("lastClr").and_then(hex),
            "schemeClr" if val == "phClr" => placeholder,
            "schemeClr" => {
                let key = self.map.get(val).map(String::as_str).unwrap_or(val);
                let source = self
                    .element("clrScheme")
                    .and_then(|s| child(s, A, key))
                    .and_then(|s| s.children().find(|c| c.is_element()));
                source.and_then(|c| self.resolve(c, None, depth + 1, problems))
            }
            "scrgbClr" => {
                let r = percentage(n.attribute("r"))?;
                let g = percentage(n.attribute("g"))?;
                let b = percentage(n.attribute("b"))?;
                Some([gamma(r), gamma(g), gamma(b), 1.0])
            }
            "hslClr" => {
                let h = n
                    .attribute("hue")
                    .and_then(|s| s.parse::<f64>().ok())
                    .filter(|v| v.is_finite())?
                    / 21600000.0;
                let rgb = hsl_to_rgb(
                    h,
                    percentage(n.attribute("sat"))?,
                    percentage(n.attribute("lum"))?,
                );
                Some([rgb[0], rgb[1], rgb[2], 1.0])
            }
            "prstClr" => {
                let name = match val {
                    "dkBlue" => "darkblue",
                    "ltBlue" => "lightblue",
                    "dkGray" => "darkgray",
                    "ltGray" => "lightgray",
                    _ => val,
                };
                name.parse::<svgtypes::Color>().ok().map(|c| {
                    [
                        c.red as f64 / 255.0,
                        c.green as f64 / 255.0,
                        c.blue as f64 / 255.0,
                        1.0,
                    ]
                })
            }
            _ => None,
        };
        if rgba.is_none() {
            problems.insert(format!(
                "color cannot be resolved: {} {val}",
                n.tag_name().name()
            ));
        }
        let c = rgba.as_mut()?;
        for transform in n.children().filter(|x| x.is_element()) {
            let name = transform.tag_name().name();
            let value = if ["comp", "inv", "gray", "gamma", "invGamma"].contains(&name) {
                0.0
            } else if ["hue", "hueOff"].contains(&name) {
                transform
                    .attribute("val")
                    .and_then(|s| s.parse::<f64>().ok())
                    .filter(|v| v.is_finite())
                    .map(|v| v / 21600000.0)?
            } else {
                let Some(v) = percentage(transform.attribute("val")) else {
                    problems.insert(format!("invalid color transform: {name}"));
                    return None;
                };
                v
            };
            match name {
                "alpha" => c[3] = value,
                "alphaMod" => c[3] *= value,
                "alphaOff" => c[3] += value,
                "tint" => {
                    for x in &mut c[..3] {
                        *x = gamma(linear(*x) * value + (1.0 - value));
                    }
                }
                "shade" => {
                    for x in &mut c[..3] {
                        *x = gamma(linear(*x) * value);
                    }
                }
                "red" | "green" | "blue" | "redMod" | "greenMod" | "blueMod" | "redOff"
                | "greenOff" | "blueOff" => {
                    let index = if name.starts_with("red") {
                        0
                    } else if name.starts_with("green") {
                        1
                    } else {
                        2
                    };
                    let channel = linear(c[index]);
                    if name.ends_with("Mod") {
                        c[index] = gamma(channel * value);
                    } else if name.ends_with("Off") {
                        c[index] = gamma(channel + value);
                    } else {
                        c[index] = gamma(value);
                    }
                }
                "hue" | "hueMod" | "hueOff" | "sat" | "satMod" | "satOff" | "lum" | "lumMod"
                | "lumOff" | "comp" => {
                    let (mut h, mut s, mut l) = rgb_to_hsl([c[0], c[1], c[2]]);
                    match name {
                        "hue" => h = value,
                        "hueMod" => h *= value,
                        "hueOff" => h += value,
                        "comp" => h += 0.5,
                        "sat" => s = value,
                        "satMod" => s *= value,
                        "satOff" => s += value,
                        "lum" => l = value,
                        "lumMod" => l *= value,
                        "lumOff" => l += value,
                        _ => unreachable!(),
                    }
                    c[..3].copy_from_slice(&hsl_to_rgb(h, s.clamp(0.0, 1.0), l.clamp(0.0, 1.0)));
                }
                "inv" => {
                    for x in &mut c[..3] {
                        *x = gamma(1.0 - linear(*x));
                    }
                }
                "gray" => {
                    let g = 0.22 * c[0] + 0.72 * c[1] + 0.06 * c[2];
                    c[..3].fill(g);
                }
                "gamma" => {
                    for x in &mut c[..3] {
                        *x = gamma(*x);
                    }
                }
                "invGamma" => {
                    for x in &mut c[..3] {
                        *x = linear(*x);
                    }
                }
                other => {
                    problems.insert(format!("color transform {other} is not reconstructed"));
                    return None;
                }
            }
            for channel in c.iter_mut() {
                *channel = channel.clamp(0.0, 1.0);
            }
        }
        rgba
    }
}
pub(super) fn color_hex(color: Color) -> String {
    if color.3 == 255 {
        color.hex()
    } else {
        format!("{}{:02X}", color.hex(), color.3)
    }
}
fn hex(value: &str) -> Option<[f64; 4]> {
    if value.len() != 6 {
        return None;
    }
    Color::parse(&format!("#{value}")).ok().map(channels)
}
fn channels(c: Color) -> [f64; 4] {
    [
        c.0 as f64 / 255.0,
        c.1 as f64 / 255.0,
        c.2 as f64 / 255.0,
        c.3 as f64 / 255.0,
    ]
}
pub(super) fn percentage(value: Option<&str>) -> Option<f64> {
    let value = value?;
    let (number, divisor) = value
        .strip_suffix('%')
        .map_or((value, 100000.0), |n| (n, 100.0));
    number
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .map(|v| v / divisor)
}
fn gamma(c: f64) -> f64 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}
fn linear(c: f64) -> f64 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}
fn rgb_to_hsl(c: [f64; 3]) -> (f64, f64, f64) {
    let max = c.into_iter().fold(f64::NEG_INFINITY, f64::max);
    let min = c.into_iter().fold(f64::INFINITY, f64::min);
    let l = (max + min) / 2.0;
    let delta = max - min;
    if delta < 1e-12 {
        return (0.0, 0.0, l);
    }
    let s = delta / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == c[0] {
        (c[1] - c[2]) / delta
    } else if max == c[1] {
        (c[2] - c[0]) / delta + 2.0
    } else {
        (c[0] - c[1]) / delta + 4.0
    };
    ((h / 6.0).rem_euclid(1.0), s, l)
}
fn hsl_to_rgb(h: f64, s: f64, l: f64) -> [f64; 3] {
    let h = h.rem_euclid(1.0) * 6.0;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let p = match h as u8 {
        0 => [c, x, 0.0],
        1 => [x, c, 0.0],
        2 => [0.0, c, x],
        3 => [0.0, x, c],
        4 => [x, 0.0, c],
        _ => [c, 0.0, x],
    };
    p.map(|v| v + m)
}
