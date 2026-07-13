//! Replicate: the primary v0.1 provider. Generic runner for any
//! `owner/model:version` with JSON inputs; recommended model lists are
//! fetched from Replicate's collections API at call time rather than
//! hardcoded — this space shifts monthly.

use std::time::Duration;

use serde::Deserialize;

use crate::error::{Error, Result};

use super::{
    GeneratedOutput, GenerationRequest, GenerationResult, ModelInfo, OutputKind, Provider,
};

pub struct Replicate {
    base_url: String,
    poll_interval: Duration,
    max_wait: Duration,
    client: reqwest::blocking::Client,
}

impl Default for Replicate {
    fn default() -> Self {
        Self::new()
    }
}

impl Replicate {
    pub fn new() -> Self {
        Self::with_base_url("https://api.replicate.com")
    }

    /// Test hook: point the client at a mock server and poll fast.
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            poll_interval: Duration::from_secs(1),
            max_wait: Duration::from_secs(15 * 60),
            client: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
        }
    }

    pub fn with_polling(mut self, interval: Duration, max_wait: Duration) -> Self {
        self.poll_interval = interval;
        self.max_wait = max_wait;
        self
    }

    fn create_prediction(&self, api_key: &str, request: &GenerationRequest) -> Result<Prediction> {
        let (url, body) = match request.model.split_once(':') {
            // Versioned community model: the generic predictions endpoint.
            Some((_, version)) => (
                format!("{}/v1/predictions", self.base_url),
                serde_json::json!({ "version": version, "input": request.inputs }),
            ),
            // Official model: the models predictions endpoint.
            None => (
                format!("{}/v1/models/{}/predictions", self.base_url, request.model),
                serde_json::json!({ "input": request.inputs }),
            ),
        };
        let response = self
            .client
            .post(&url)
            .bearer_auth(api_key)
            .header("Prefer", "wait=10")
            .json(&body)
            .send()
            .map_err(http_error)?;
        parse_prediction(response)
    }

    fn poll(&self, api_key: &str, prediction: Prediction) -> Result<Prediction> {
        let started = std::time::Instant::now();
        let mut current = prediction;
        loop {
            match current.status.as_str() {
                "succeeded" | "failed" | "canceled" => return Ok(current),
                _ => {}
            }
            if started.elapsed() > self.max_wait {
                return Err(Error::Generation(format!(
                    "generation timed out after {:?} (prediction {})",
                    self.max_wait, current.id
                )));
            }
            std::thread::sleep(self.poll_interval);
            let url = current
                .urls
                .as_ref()
                .and_then(|u| u.get.clone())
                .unwrap_or_else(|| format!("{}/v1/predictions/{}", self.base_url, current.id));
            let response = self
                .client
                .get(&url)
                .bearer_auth(api_key)
                .send()
                .map_err(http_error)?;
            current = parse_prediction(response)?;
        }
    }

    fn download(&self, api_key: &str, url: &str) -> Result<GeneratedOutput> {
        let response = self
            .client
            .get(url)
            .bearer_auth(api_key)
            .send()
            .map_err(http_error)?;
        if !response.status().is_success() {
            return Err(Error::Generation(format!(
                "downloading output failed with HTTP {} ({url})",
                response.status()
            )));
        }
        let extension = extension_from_url(url)
            .or_else(|| {
                response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .and_then(extension_from_content_type)
            })
            .unwrap_or_else(|| "bin".to_owned());
        let bytes = response.bytes().map_err(http_error)?.to_vec();
        Ok(GeneratedOutput {
            kind: OutputKind::from_extension(&extension),
            extension,
            bytes,
        })
    }
}

impl Provider for Replicate {
    fn id(&self) -> &'static str {
        "replicate"
    }

    fn display_name(&self) -> &'static str {
        "Replicate"
    }

    fn generate(&self, api_key: &str, request: &GenerationRequest) -> Result<GenerationResult> {
        let prediction = self.create_prediction(api_key, request)?;
        let finished = self.poll(api_key, prediction)?;
        match finished.status.as_str() {
            "succeeded" => {
                let urls = output_urls(&finished.output);
                if urls.is_empty() {
                    return Err(Error::Generation(format!(
                        "prediction {} succeeded but produced no output files",
                        finished.id
                    )));
                }
                let mut outputs = Vec::new();
                for url in &urls {
                    outputs.push(self.download(api_key, url)?);
                }
                Ok(GenerationResult {
                    outputs,
                    cost_usd: None,
                    provider_meta: finished.raw,
                })
            }
            status => Err(Error::Generation(format!(
                "prediction {} {status}: {}",
                finished.id,
                finished.error.unwrap_or_else(|| "no detail".to_owned())
            ))),
        }
    }

    fn recommended_models(&self, api_key: &str) -> Result<Vec<ModelInfo>> {
        let mut models = Vec::new();
        for (collection, kind) in [
            ("text-to-video", OutputKind::Video),
            ("image-to-video", OutputKind::Video),
            ("text-to-image", OutputKind::Image),
        ] {
            let url = format!("{}/v1/collections/{collection}", self.base_url);
            let response = match self.client.get(&url).bearer_auth(api_key).send() {
                Ok(r) if r.status().is_success() => r,
                _ => continue, // collections come and go; best effort
            };
            let Ok(body) = response.json::<CollectionResponse>() else {
                continue;
            };
            for model in body.models.into_iter().take(8) {
                models.push(ModelInfo {
                    slug: format!("{}/{}", model.owner, model.name),
                    version: model.latest_version.map(|v| v.id),
                    display_name: model.name.replace('-', " "),
                    description: model.description,
                    kind: kind.clone(),
                });
            }
        }
        Ok(models)
    }
}

#[derive(Debug)]
struct Prediction {
    id: String,
    status: String,
    output: serde_json::Value,
    error: Option<String>,
    urls: Option<PredictionUrls>,
    raw: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct PredictionUrls {
    get: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CollectionResponse {
    #[serde(default)]
    models: Vec<CollectionModel>,
}

#[derive(Debug, Deserialize)]
struct CollectionModel {
    owner: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    latest_version: Option<VersionRef>,
}

#[derive(Debug, Deserialize)]
struct VersionRef {
    id: String,
}

fn parse_prediction(response: reqwest::blocking::Response) -> Result<Prediction> {
    let status = response.status();
    let raw: serde_json::Value = response.json().map_err(http_error)?;
    if !status.is_success() {
        let detail = raw
            .get("detail")
            .and_then(|d| d.as_str())
            .unwrap_or("no detail");
        return Err(Error::Generation(format!(
            "Replicate returned HTTP {status}: {detail}"
        )));
    }
    Ok(Prediction {
        id: raw
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned(),
        status: raw
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_owned(),
        output: raw
            .get("output")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        error: raw.get("error").and_then(|v| v.as_str()).map(str::to_owned),
        urls: raw
            .get("urls")
            .and_then(|u| serde_json::from_value(u.clone()).ok()),
        raw,
    })
}

/// Replicate output shapes: a single URL string, an array of URL strings,
/// or an object with URL values. Collect anything that looks like a URL.
fn output_urls(output: &serde_json::Value) -> Vec<String> {
    fn collect(value: &serde_json::Value, out: &mut Vec<String>) {
        match value {
            serde_json::Value::String(s) if s.starts_with("http") => out.push(s.clone()),
            serde_json::Value::Array(items) => {
                for item in items {
                    collect(item, out);
                }
            }
            serde_json::Value::Object(map) => {
                for item in map.values() {
                    collect(item, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    collect(output, &mut out);
    out
}

fn extension_from_url(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let name = path.rsplit('/').next()?;
    let (_, ext) = name.rsplit_once('.')?;
    ((1..=4).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()))
        .then(|| ext.to_ascii_lowercase())
}

fn extension_from_content_type(content_type: &str) -> Option<String> {
    let essence = content_type.split(';').next().unwrap_or("").trim();
    let ext = match essence {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/quicktime" => "mov",
        "audio/mpeg" => "mp3",
        "audio/wav" | "audio/x-wav" => "wav",
        _ => return None,
    };
    Some(ext.to_owned())
}

fn http_error(e: reqwest::Error) -> Error {
    Error::Generation(format!("network request failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_urls_handles_all_shapes() {
        let single = serde_json::json!("https://x/a.png");
        assert_eq!(output_urls(&single), ["https://x/a.png"]);
        let array = serde_json::json!(["https://x/a.png", "https://x/b.mp4"]);
        assert_eq!(output_urls(&array).len(), 2);
        let object = serde_json::json!({"video": "https://x/c.mp4", "seed": 42});
        assert_eq!(output_urls(&object), ["https://x/c.mp4"]);
        assert!(output_urls(&serde_json::json!(null)).is_empty());
    }

    #[test]
    fn extensions_come_from_urls_and_content_types() {
        assert_eq!(
            extension_from_url("https://x/files/out.mp4?token=abc"),
            Some("mp4".to_owned())
        );
        assert_eq!(extension_from_url("https://x/files/noext"), None);
        assert_eq!(
            extension_from_content_type("image/png"),
            Some("png".to_owned())
        );
        assert_eq!(
            extension_from_content_type("video/mp4; charset=binary"),
            Some("mp4".to_owned())
        );
    }
}
