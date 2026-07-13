use std::path::PathBuf;

/// Errors produced by autoteur-core.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid {kind} {value:?}: expected {expected}")]
    InvalidId {
        kind: &'static str,
        value: String,
        expected: &'static str,
    },

    #[error("failed to read {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("TOML syntax error")]
    Syntax(#[source] Box<toml_edit::TomlError>),

    #[error("schema error")]
    Schema(#[source] Box<toml_edit::de::Error>),

    #[error("edit failed: {0}")]
    Edit(String),
}

pub type Result<T> = std::result::Result<T, Error>;
