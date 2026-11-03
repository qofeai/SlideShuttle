use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceError {
    /// Project-relative file; empty for a single source string.
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub message: String,
}
impl std::fmt::Display for SourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.file.is_empty() {
            write!(f, "{} ", self.file)?;
        }
        write!(
            f,
            "line {}, column {}: {}",
            self.line, self.column, self.message
        )
    }
}
/// Attach a source position unless the error is already located.
pub(crate) fn at(source: &Source, error: Error) -> Error {
    match error {
        Error::Source(_) => error,
        other => Error::Source(vec![SourceError {
            file: source.file.clone(),
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
    /// Project-relative file; empty for a single source string.
    pub file: String,
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
        paragraphs: Vec<Paragraph>,
    },
    Image {
        src: String,
        /// Clip geometry; `rect` with border-radius for rounded corners.
        clip: ShapeKind,
    },
    /// A single-color SVG icon drawn in the text color.
    Icon {
        src: String,
    },
    Shape {
        shape: ShapeKind,
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
        chart_type: ChartType,
        labels: Vec<String>,
        series: Vec<Series>,
    },
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FitMode {
    #[default]
    Contain,
    Cover,
    Fill,
}
/// `object-fit` and `object-position`: where the image sits in the free space, 0 to 1 per axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageFit {
    pub mode: FitMode,
    pub position: [f32; 2],
}
impl Default for ImageFit {
    fn default() -> Self {
        Self {
            mode: FitMode::Contain,
            position: [0.5, 0.5],
        }
    }
}
/// An OOXML preset geometry (`rect`, `ellipse`, `line`, `rightArrow`, `star5`, …) and its
/// adjust values in DrawingML units (usually 1/100000 of the frame).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShapeKind {
    pub preset: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub adjust: Vec<f64>,
}
impl Default for ShapeKind {
    fn default() -> Self {
        Self {
            preset: "rect".into(),
            adjust: Vec::new(),
        }
    }
}
impl ShapeKind {
    pub fn is(&self, preset: &str) -> bool {
        self.preset == preset
    }
    /// Lines and connectors: their border is a stroke along open geometry.
    pub fn is_open(&self) -> bool {
        crate::preset::is_open(&self.preset)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChartType {
    Bar,
    Line,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    Left,
    Center,
    Right,
    Justify,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VerticalAlign {
    #[default]
    Top,
    Middle,
    Bottom,
}
#[derive(Clone, Debug)]
pub struct TextMark {
    pub rect: Rect,
    pub color: Color,
    pub kind: MarkKind,
}
#[derive(Clone, Debug, PartialEq)]
pub enum MarkKind {
    /// Painted behind the glyphs.
    Highlight,
    /// Underline or strikethrough, painted over the glyphs.
    Line,
    /// Clickable area of a link; not painted.
    Link(String),
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}
/// Parse a lowercase keyword into one of the enums above.
pub(crate) fn keyword<T: serde::de::DeserializeOwned>(value: &str, what: &str) -> Result<T> {
    serde_json::from_value(serde_json::Value::String(value.into()))
        .map_err(|e| invalid(format!("{what}: {e}")))
}
/// One paragraph of a Text: inline spans, paragraph style (`text-align`, `margin-top`)
/// and an optional list marker.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Paragraph {
    pub spans: Vec<Span>,
    pub style: Css,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list: Option<ListMarker>,
}
impl Paragraph {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            spans: vec![Span {
                text: text.into(),
                style: Css::new(),
                href: None,
            }],
            style: Css::new(),
            list: None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ListMarker {
    pub ordered: bool,
    /// Nesting depth, 0 for a top-level item.
    pub level: u8,
    /// Item number for ordered lists.
    pub number: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Span {
    pub text: String,
    pub style: Css,
    /// External link target (http, https or mailto).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
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
    pub severity: Severity,
    pub page: String,
    pub node_id: String,
    pub message: String,
    pub source: Option<Source>,
}
#[derive(Clone, Debug)]
pub struct PositionedGlyph {
    /// Index into `Resources::fonts` of the file that was shaped against.
    pub font: usize,
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
    pub align: Align,
    /// Extra space after each character, in px.
    pub letter_spacing: f32,
    pub underline: bool,
    pub strike: bool,
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
            align: Align::Left,
            letter_spacing: 0.0,
            underline: false,
            strike: false,
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
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }
}
#[derive(Clone, Debug)]
pub struct ScenePage {
    pub id: String,
    /// Full-page item: a solid or gradient Box, or an Image covering the page.
    pub background: Item,
    pub items: Vec<Item>,
}
#[derive(Clone, Debug)]
pub struct Item {
    pub id: String,
    pub rect: Rect,
    pub fill: Color,
    pub gradient: Option<Gradient>,
    pub stroke: Color,
    pub stroke_width: f32,
    pub dash: Dash,
    pub line_cap: LineCap,
    pub line_join: LineJoin,
    /// Line end decorations at the start and end of a line or path.
    pub markers: [Option<Marker>; 2],
    pub radius: f32,
    pub opacity: f32,
    pub shadow: Option<Shadow>,
    pub turn: Turn,
    pub group: Option<String>,
    pub kind: Draw,
    pub text_style: TextStyle,
    pub source: Option<Source>,
}
impl Default for Item {
    fn default() -> Self {
        Self {
            id: String::new(),
            rect: Rect::default(),
            fill: Color::default(),
            gradient: None,
            stroke: Color::BLACK,
            stroke_width: 0.0,
            dash: Dash::Solid,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
            markers: [None, None],
            radius: 0.0,
            opacity: 1.0,
            shadow: None,
            turn: Turn::default(),
            group: None,
            kind: Draw::Box,
            text_style: TextStyle::default(),
            source: None,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    pub radial: bool,
    /// Linear direction as a CSS angle: 0 points up, 90 right, clockwise.
    pub angle: f32,
    /// Positions in [0, 1], non-decreasing.
    pub stops: Vec<(f32, Color)>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Dash {
    #[default]
    Solid,
    Dashed,
    Dotted,
}
/// `stroke-linecap`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineCap {
    #[default]
    Butt,
    Round,
    Square,
}
/// `stroke-linejoin`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineJoin {
    #[default]
    Miter,
    Round,
    Bevel,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Marker {
    Arrow,
    Stealth,
    Diamond,
    Oval,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub dx: f32,
    pub dy: f32,
    pub blur: f32,
    pub color: Color,
}
/// Rotation (degrees, clockwise) and mirroring about the item's center; does not affect layout.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Turn {
    pub rotate: f32,
    pub flip_x: bool,
    pub flip_y: bool,
}
impl Turn {
    pub fn is_identity(self) -> bool {
        self == Self::default()
    }
}
#[derive(Clone, Debug)]
pub enum Draw {
    Box,
    Group,
    Shape(ShapeKind),
    Path {
        data: String,
        view_box: [f32; 2],
    },
    Text {
        paragraphs: Vec<Paragraph>,
        glyphs: Vec<PositionedGlyph>,
        /// Highlights, decoration lines and link areas, in page coordinates.
        marks: Vec<TextMark>,
        line_count: usize,
        /// Whether another layout engine may insert soft line breaks.
        export_wrap: bool,
        anchor: VerticalAlign,
    },
    Image {
        src: String,
        fit: ImageFit,
        clip: ShapeKind,
    },
    Icon(crate::icon::Icon),
    Table {
        rows: Vec<Vec<String>>,
        columns: Vec<f32>,
        cells: Vec<Item>,
    },
    Chart {
        chart_type: ChartType,
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
