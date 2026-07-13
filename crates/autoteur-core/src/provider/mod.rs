//! Generation providers. A `Provider` turns a request (model + JSON
//! inputs) into output media, synchronously from the caller's point of
//! view — the job queue supplies the threads. Every generation's full
//! parameters are recorded in the takes manifest by the queue, so any
//! take can be re-printed from the negative.

pub mod replicate;
pub mod secrets;
pub mod stubs;

use std::sync::Arc;

use crate::error::Result;

/// A generation request: the model reference and the provider's full
/// inputs, verbatim (this exact value lands in the manifest).
#[derive(Debug, Clone)]
pub struct GenerationRequest {
    /// `owner/name` or `owner/name:version` (provider-specific).
    pub model: String,
    pub inputs: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputKind {
    Image,
    Video,
    Audio,
    Other(String),
}

impl OutputKind {
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_ascii_lowercase().as_str() {
            "png" | "jpg" | "jpeg" | "webp" | "gif" => Self::Image,
            "mp4" | "webm" | "mov" | "mkv" => Self::Video,
            "mp3" | "wav" | "flac" | "ogg" => Self::Audio,
            other => Self::Other(other.to_owned()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Image => "image",
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Other(s) => s,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GeneratedOutput {
    pub bytes: Vec<u8>,
    pub kind: OutputKind,
    /// File extension without the dot, e.g. `mp4`.
    pub extension: String,
}

#[derive(Debug, Clone)]
pub struct GenerationResult {
    pub outputs: Vec<GeneratedOutput>,
    pub cost_usd: Option<f64>,
    /// Raw provider response for the Under-the-Hood flap.
    pub provider_meta: serde_json::Value,
}

/// A model a provider recommends for a task, fetched dynamically — this
/// space shifts monthly, so nothing is hardcoded.
#[derive(Debug, Clone)]
pub struct ModelInfo {
    /// `owner/name`, ready to use in a GenerationRequest.
    pub slug: String,
    pub version: Option<String>,
    pub display_name: String,
    pub description: Option<String>,
    pub kind: OutputKind,
}

pub trait Provider: Send + Sync {
    /// Stable id used for key storage and job routing, e.g. "replicate".
    fn id(&self) -> &'static str;
    /// Friendly name for Studio Settings, e.g. "Replicate".
    fn display_name(&self) -> &'static str;
    fn generate(&self, api_key: &str, request: &GenerationRequest) -> Result<GenerationResult>;
    /// Current recommended models (best-effort; may be empty).
    fn recommended_models(&self, _api_key: &str) -> Result<Vec<ModelInfo>> {
        Ok(Vec::new())
    }
}

#[derive(Clone)]
pub struct ProviderRegistry {
    providers: Vec<Arc<dyn Provider>>,
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self {
            providers: vec![
                Arc::new(replicate::Replicate::new()),
                Arc::new(stubs::OpenAiImages),
                Arc::new(stubs::AnthropicText),
                Arc::new(stubs::ElevenLabsVoice),
                Arc::new(stubs::FalAi),
            ],
        }
    }
}

impl ProviderRegistry {
    pub fn with_providers(providers: Vec<Arc<dyn Provider>>) -> Self {
        Self { providers }
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn Provider>> {
        self.providers.iter().find(|p| p.id() == id).cloned()
    }

    pub fn all(&self) -> &[Arc<dyn Provider>] {
        &self.providers
    }
}
