//! Cross-platform compositor. The engine has no UI dependency beyond what the
//! binary pulls in; pixel work, the `.comp` package and the editor window live
//! side by side so the same document can be tested without opening a window.

mod blend;
mod decode;
mod document;
mod effects;
mod edit;
mod ops;
mod project;
mod psd;
mod raster;
mod render;
mod svg;

mod app;

pub use app::run;
