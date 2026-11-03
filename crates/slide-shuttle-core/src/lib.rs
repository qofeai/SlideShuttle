//! Independent declarative presentation engine. Resources are supplied as bytes.
pub mod baseline;
mod chart;
pub mod compiler;
mod font_fallback;
pub mod icon;
pub mod import;
pub mod layout;
pub mod model;
pub mod parser;
pub mod pptx;
pub mod preset;
pub mod render;
mod table;
mod text;
pub use compiler::{CompiledDocument, Compiler, RenderStats, TextMeasurement};
pub use layout::layout;
pub use model::*;
pub use parser::{parse, parse_files, parse_project};
