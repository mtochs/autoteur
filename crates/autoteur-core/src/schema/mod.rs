//! Typed views of every Autoteur project file. Parsing is lenient where the
//! format spec says so (unknown keys ignored, optional fields defaulted) and
//! strict where identity is at stake (ids, references, take pointers).

pub mod beats;
pub mod character;
pub mod common;
pub mod project;
pub mod scene;
pub mod shots;
pub mod takes;
pub mod timeline;
pub mod world;
