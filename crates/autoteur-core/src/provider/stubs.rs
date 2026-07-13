//! Stub providers: the trait is proven, the bodies are honest TODOs. Each
//! returns a clear "not wired up yet" error instead of pretending.

use crate::error::{Error, Result};

use super::{GenerationRequest, GenerationResult, Provider};

macro_rules! stub_provider {
    ($ty:ident, $id:literal, $name:literal) => {
        pub struct $ty;

        impl Provider for $ty {
            fn id(&self) -> &'static str {
                $id
            }
            fn display_name(&self) -> &'static str {
                $name
            }
            fn generate(
                &self,
                _api_key: &str,
                _request: &GenerationRequest,
            ) -> Result<GenerationResult> {
                Err(Error::Generation(format!(
                    "{} isn't wired up yet — Replicate is the v0.1 provider",
                    $name
                )))
            }
        }
    };
}

stub_provider!(OpenAiImages, "openai", "OpenAI Images");
stub_provider!(AnthropicText, "anthropic", "Anthropic");
stub_provider!(ElevenLabsVoice, "elevenlabs", "ElevenLabs");
stub_provider!(FalAi, "fal", "fal.ai");
