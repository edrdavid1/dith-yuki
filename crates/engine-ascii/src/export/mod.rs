//! Exporters: pure functions of [`AsciiGrid`] + atlas → text / files.

mod ansi;
mod cell_paint;
mod html;
mod json;
mod png;
mod svg;
mod txt;

pub use ansi::{parse_ansi_indices, to_ansi, AnsiDepth, AnsiOptions};
pub use html::{to_html, HtmlOptions};
pub use json::to_json;
pub use png::{to_png, to_png_scaled};
pub use svg::{to_svg, SvgOptions};
pub use txt::{to_txt, TXT_EXPORT_MAX_COLS};
