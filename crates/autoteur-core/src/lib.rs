//! autoteur-core — domain types, file formats, prompt resolution, and
//! validation for Autoteur projects.
//!
//! An Autoteur project is a plain git repository of TOML and Markdown; this
//! crate is the single definition of those formats for the GUI, the CLI,
//! and every test. Format specification: `docs/proposals/0001`.

#![cfg_attr(not(test), warn(clippy::unwrap_used))]

pub mod doc;
pub mod error;
pub mod id;
pub mod lint;
pub mod prompt;
pub mod schema;

pub use error::{Error, Result};
// Re-exported so downstream crates edit documents with the same version.
pub use toml_edit;
