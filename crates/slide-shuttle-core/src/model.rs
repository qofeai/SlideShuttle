use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const FORMAT_VERSION: &str = "1";
pub const EMU_PER_PX: f64 = 9525.0;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("XML: {0}")]
    Xml(#[from] roxmltree::Error),
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("ZIP: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("{}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"))]
    Source(Vec<SourceError>),
}
/// An authoring error tied to the element that caused it.
#[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
#[error("line {line}, column {column}: {message}")]
pub struct SourceError {
    pub line: u32,
    pub column: u32,
    pub message: String,
}
/// Attach a source position unless the error is already located.
pub(crate) fn at(source: &Source, error: Error) -> Error {
    match error {
        Error::Source(_) => error,
        other => Error::Source(vec![SourceError {
            line: source.line,
            column: source.column,
            message: other.to_string(),
        }]),
    }
}
pub type Result<T> = std::result::Result<T, Error>;
pub(crate) fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}

pub type Css = BTreeMap<String, String>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Deck {
    pub version: String,
    pub title: String,
    pub width: f32,
    pub height: f32,
    pub font_family: String,
    pub fonts: Vec<FontSpec>,
    pub assets: BTreeMap<String, String>,
    pub slides: Vec<Slide>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FontSpec {
    pub family: String,
    pub src: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Slide {
    pub id: String,
    pub background: String,
    pub nodes: Vec<Node>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Source {
    pub line: u32,
    pub column: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub style: Css,
    pub kind: Kind,
    pub children: Vec<Node>,
    pub source: Source,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Kind {
    Box,
    Group,
    Text {
        spans: Vec<Span>,
    },
    Image {
        src: String,
    },
    Shape {
        shape: String,
    },
    Path {
        data: String,
        view_box: [f32; 2],
    },
    Table {
        rows: Vec<Vec<String>>,
        columns: Vec<f32>,
    },
    Chart {
        chart_type: String,
        labels: Vec<String>,
        series: Vec<Series>,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Span {
    pub text: String,
    pub style: Css,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Series {
    pub name: String,
    pub values: Vec<f64>,
    pub color: String,
}

#[derive(Clone, Debug)]
pub struct FontResource {
    /// Family the file must provide; `None` loads the file without a declared family.
    pub family: Option<String>,
    pub bytes: Vec<u8>,
}
#[derive(Clone, Debug, Default)]
pub struct Resources {
    pub fonts: Vec<FontResource>,
    pub images: BTreeMap<String, Vec<u8>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
impl Rect {
    pub fn right(self) -> f32 {
        self.x + self.width
    }
    pub fn bottom(self) -> f32 {
        self.y + self.height
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Color(pub u8, pub u8, pub u8, pub u8);
impl Color {
    pub const BLACK: Self = Self(0, 0, 0, 255);
    pub const WHITE: Self = Self(255, 255, 255, 255);
    pub fn hex(self) -> String {
        format!("{:02X}{:02X}{:02X}", self.0, self.1, self.2)
    }
    pub fn parse(value: &str) -> Result<Self> {
        let s = value.trim();
        match s {
            "transparent" => return Ok(Self(0, 0, 0, 0)),
            "white" => return Ok(Self::WHITE),
            "black" => return Ok(Self::BLACK),
            _ => {}
        }
        let h = s
            .strip_prefix('#')
            .ok_or_else(|| invalid(format!("expected hex color, got {s}")))?;
        let expanded = if h.len() == 3 {
            h.chars().flat_map(|c| [c, c]).collect::<String>()
        } else {
            h.to_owned()
        };
        if ![6, 8].contains(&expanded.len()) || !expanded.is_ascii() {
            return Err(invalid(format!("invalid color {s}")));
        }
        let v = u32::from_str_radix(&expanded, 16)
            .map_err(|_| invalid(format!("invalid color {s}")))?;
        Ok(if expanded.len() == 8 {
            Self((v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8, v as u8)
        } else {
            Self((v >> 16) as u8, (v >> 8) as u8, v as u8, 255)
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub code: String,
    pub severity: String,
    pub page: String,
    pub node_id: String,
    pub message: String,
    pub source: Option<Source>,
}
#[derive(Clone, Debug)]
pub struct PositionedGlyph {
    pub font_key: String,
    pub font_index: u32,
    pub glyph_id: u16,
    pub size: f32,
    pub weight: u16,
    pub x: f32,
    pub y: f32,
    pub advance: f32,
    pub text: String,
    pub color: Color,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct TextStyle {
    pub family: String,
    pub size: f32,
    pub line_height: f32,
    pub weight: u16,
    pub italic: bool,
    pub color: Color,
    pub align: String,
}
impl Default for TextStyle {
    fn default() -> Self {
        Self {
            family: "Noto Sans CJK SC".into(),
            size: 24.0,
            line_height: 32.0,
            weight: 400,
            italic: false,
            color: Color::BLACK,
            align: "left".into(),
        }
    }
}
#[derive(Clone, Debug)]
pub struct Scene {
    pub width: f32,
    pub height: f32,
    pub title: String,
    pub pages: Vec<ScenePage>,
    pub diagnostics: Vec<Diagnostic>,
}
impl Scene {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == "error")
    }
}
#[derive(Clone, Debug)]
pub struct ScenePage {
    pub id: String,
    pub background: Color,
    pub items: Vec<Item>,
}
#[derive(Clone, Debug)]
pub struct Item {
    pub id: String,
    pub rect: Rect,
    pub fill: Color,
    pub gradient: Option<LinearGradient>,
    pub stroke: Color,
    pub stroke_width: f32,
    pub radius: f32,
    pub opacity: f32,
    pub group: Option<String>,
    pub kind: Draw,
    pub text_style: TextStyle,
    pub source: Option<Source>,
}
#[derive(Clone, Copy, Debug)]
pub struct LinearGradient {
    pub vertical: bool,
    pub start: Color,
    pub end: Color,
}
#[derive(Clone, Debug)]
pub enum Draw {
    Box,
    Group,
    Shape(String),
    Path {
        data: String,
        view_box: [f32; 2],
    },
    Text {
        spans: Vec<Span>,
        glyphs: Vec<PositionedGlyph>,
        line_count: usize,
        /// Whether another layout engine may insert soft line breaks.
        export_wrap: bool,
    },
    Image {
        src: String,
        fit: String,
    },
    Table {
        rows: Vec<Vec<String>>,
        columns: Vec<f32>,
        cells: Vec<Item>,
    },
    Chart {
        chart_type: String,
        labels: Vec<String>,
        series: Vec<Series>,
        primitives: Vec<Item>,
    },
}

pub(crate) fn finite(v: f32, context: &str) -> Result<f32> {
    if v.is_finite() {
        Ok(v)
    } else {
        Err(invalid(format!("non-finite number in {context}")))
    }
}
pub fn length_px(value: &str) -> Result<f32> {
    let s = value.trim();
    let (n, k) = if let Some(n) = s.strip_suffix("emu") {
        (n, 1.0 / EMU_PER_PX as f32)
    } else if let Some(n) = s.strip_suffix("pt") {
        (n, 4.0 / 3.0)
    } else if let Some(n) = s.strip_suffix("px") {
        (n, 1.0)
    } else if s == "0" {
        (s, 1.0)
    } else {
        return Err(invalid(format!("expected px, pt or emu length; got {s}")));
    };
    finite(
        n.trim()
            .parse::<f32>()
            .map_err(|_| invalid(format!("invalid length {s}")))?
            * k,
        s,
    )
}
pub fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}
pub(crate) fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
