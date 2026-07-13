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

    #[error("{path} is not UTF-8 text{hint}")]
    Encoding {
        path: PathBuf,
        /// e.g. " (it looks like UTF-16 — rewrite it as UTF-8)"
        hint: String,
    },

    #[error("git operation failed")]
    Git(#[source] Box<git2::Error>),

    #[error("file watcher failed: {0}")]
    Watch(String),

    #[error("{0}")]
    Generation(String),

    #[error("credential store error: {0}")]
    Secret(String),

    #[error("{0}")]
    Project(String),
}

impl From<git2::Error> for Error {
    fn from(e: git2::Error) -> Self {
        Error::Git(Box::new(e))
    }
}

pub type Result<T> = std::result::Result<T, Error>;
