//! Generation pipeline: queue → provider → content-addressed store →
//! manifest append, plus the Replicate client against a mock server.

use std::fs;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Duration;

use autoteur_core::error::Result;
use autoteur_core::id::ShotRef;
use autoteur_core::project::Project;
use autoteur_core::provider::replicate::Replicate;
use autoteur_core::provider::{
    GeneratedOutput, GenerationRequest, GenerationResult, OutputKind, Provider, ProviderRegistry,
};
use autoteur_core::queue::{GenerationJob, GenerationQueue, JobStage, JobUpdate};
use autoteur_core::schema::project::ProjectFormat;

struct FakeProvider {
    bytes: Vec<u8>,
}

impl Provider for FakeProvider {
    fn id(&self) -> &'static str {
        "fake"
    }
    fn display_name(&self) -> &'static str {
        "Fake Studio"
    }
    fn generate(&self, _api_key: &str, _request: &GenerationRequest) -> Result<GenerationResult> {
        Ok(GenerationResult {
            outputs: vec![GeneratedOutput {
                bytes: self.bytes.clone(),
                kind: OutputKind::Video,
                extension: "mp4".to_owned(),
            }],
            cost_usd: Some(0.02),
            provider_meta: serde_json::json!({"fake": true}),
        })
    }
}

fn wait_done(rx: &Receiver<JobUpdate>) -> JobStage {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .expect("job finished in time");
        let update = rx.recv_timeout(remaining).expect("update");
        match update.stage {
            JobStage::Done { .. } | JobStage::Failed { .. } => return update.stage,
            _ => {}
        }
    }
}

fn job() -> GenerationJob {
    GenerationJob {
        shot: "vault/a".parse().expect("ref"),
        provider: "fake".to_owned(),
        request: GenerationRequest {
            model: "acme/dream-machine".to_owned(),
            inputs: serde_json::json!({"prompt": "the vault door", "seed": 42}),
        },
        resolved_prompt: Some("the vault door".to_owned()),
        negative_prompt: Some("watermark".to_owned()),
        seed: Some(42),
    }
}

#[test]
fn queue_end_to_end_stores_media_and_appends_manifest() {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = Project::create(dir.path(), "Gen", ProjectFormat::Feature).expect("create");
    let registry = ProviderRegistry::with_providers(vec![Arc::new(FakeProvider {
        bytes: b"FAKE MP4 BYTES".to_vec(),
    })]);
    let (queue, updates) = GenerationQueue::start(
        dir.path().to_owned(),
        registry,
        Arc::new(|_| Ok(Some("test-key".to_owned()))),
    );

    queue.submit(job());
    let stage = wait_done(&updates);
    let JobStage::Done { take, deduplicated } = stage else {
        panic!("job failed: {stage:?}");
    };
    assert!(!deduplicated);

    // Manifest: one record with the full parameters, comments preserved.
    let manifest_text =
        fs::read_to_string(dir.path().join("takes.manifest.toml")).expect("manifest");
    assert!(manifest_text.starts_with("# takes.manifest.toml"));
    let scan = project.scan();
    let takes = scan.state.takes.expect("manifest parses");
    assert_eq!(takes.data.takes.len(), 1);
    let record = &takes.data.takes[0];
    assert_eq!(record.id, take);
    assert_eq!(record.shot, "vault/a".parse::<ShotRef>().expect("ref"));
    assert_eq!(record.model, "acme/dream-machine");
    assert_eq!(record.seed, Some(42));
    assert_eq!(record.resolved_prompt.as_deref(), Some("the vault door"));
    let inputs = record.inputs.as_ref().expect("inputs recorded");
    assert_eq!(
        inputs.get("prompt").and_then(|v| v.as_str()),
        Some("the vault door")
    );
    let created = record.created_at.as_deref().expect("timestamp");
    assert!(created.ends_with('Z') && created.contains('T'));

    // Media landed content-addressed.
    let media_rel = record.outputs[0].path.as_deref().expect("path");
    let media = dir.path().join(media_rel);
    assert!(media.exists(), "{media:?}");
    assert_eq!(fs::read(&media).expect("media"), b"FAKE MP4 BYTES");

    // Bit-identical regeneration dedupes onto the same take.
    queue.submit(job());
    let stage = wait_done(&updates);
    let JobStage::Done {
        take: second,
        deduplicated,
    } = stage
    else {
        panic!("second job failed: {stage:?}");
    };
    assert_eq!(second, take);
    assert!(deduplicated);
    let scan = project.scan();
    assert_eq!(scan.state.takes.expect("manifest").data.takes.len(), 1);

    queue.shutdown();
}

#[test]
fn missing_api_key_fails_with_studio_settings_hint() {
    let dir = tempfile::tempdir().expect("tempdir");
    Project::create(dir.path(), "Gen", ProjectFormat::Feature).expect("create");
    let registry =
        ProviderRegistry::with_providers(vec![Arc::new(FakeProvider { bytes: vec![1] })]);
    let (queue, updates) =
        GenerationQueue::start(dir.path().to_owned(), registry, Arc::new(|_| Ok(None)));
    queue.submit(job());
    let JobStage::Failed { message } = wait_done(&updates) else {
        panic!("expected failure");
    };
    assert!(message.contains("connect your studio"), "{message}");
    queue.shutdown();
}

// ---------------------------------------------------------------------
// Replicate client against a mock HTTP server.
// ---------------------------------------------------------------------

fn spawn_mock_replicate() -> (String, std::thread::JoinHandle<()>) {
    let server = tiny_http::Server::http("127.0.0.1:0").expect("bind");
    let port = server.server_addr().to_ip().expect("ip").port();
    let base = format!("http://127.0.0.1:{port}");
    let base_for_server = base.clone();
    let handle = std::thread::spawn(move || {
        let mut polls = 0;
        for request in server.incoming_requests() {
            let url = request.url().to_owned();
            let respond_json = |req: tiny_http::Request, body: String| {
                let response = tiny_http::Response::from_string(body).with_header(
                    tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
                        .expect("header"),
                );
                let _ = req.respond(response);
            };
            if url.starts_with("/v1/models/acme/gen/predictions") {
                respond_json(
                    request,
                    format!(
                        r#"{{"id":"p1","status":"processing","urls":{{"get":"{base_for_server}/v1/predictions/p1"}}}}"#
                    ),
                );
            } else if url.starts_with("/v1/predictions/p1") {
                polls += 1;
                if polls < 2 {
                    respond_json(request, r#"{"id":"p1","status":"processing"}"#.to_owned());
                } else {
                    respond_json(
                        request,
                        format!(
                            r#"{{"id":"p1","status":"succeeded","output":["{base_for_server}/files/out.png"]}}"#
                        ),
                    );
                }
            } else if url.starts_with("/v1/predictions/") {
                // versioned-model flow goes straight to failed
                respond_json(
                    request,
                    r#"{"id":"p2","status":"failed","error":"NSFW content detected"}"#.to_owned(),
                );
            } else if url.starts_with("/v1/models/bad/gen/predictions") {
                respond_json(
                    request,
                    r#"{"id":"p2","status":"failed","error":"boom"}"#.to_owned(),
                );
            } else if url.starts_with("/files/out.png") {
                let response = tiny_http::Response::from_data(b"PNGBYTES".to_vec()).with_header(
                    tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"image/png"[..])
                        .expect("header"),
                );
                let _ = request.respond(response);
            } else if url.starts_with("/v1/collections/text-to-video") {
                respond_json(
                    request,
                    r#"{"models":[{"owner":"acme","name":"dream-video","description":"video model","latest_version":{"id":"v123"}}]}"#
                        .to_owned(),
                );
            } else {
                // includes unknown collections: 404s are tolerated
                let _ = request.respond(tiny_http::Response::empty(404));
            }
        }
    });
    (base, handle)
}

#[test]
fn replicate_creates_polls_and_downloads() {
    let (base, _server) = spawn_mock_replicate();
    let client = Replicate::with_base_url(&base)
        .with_polling(Duration::from_millis(10), Duration::from_secs(5));

    let result = client
        .generate(
            "key",
            &GenerationRequest {
                model: "acme/gen".to_owned(),
                inputs: serde_json::json!({"prompt": "x"}),
            },
        )
        .expect("generation succeeds");
    assert_eq!(result.outputs.len(), 1);
    assert_eq!(result.outputs[0].bytes, b"PNGBYTES");
    assert_eq!(result.outputs[0].kind, OutputKind::Image);
    assert_eq!(result.outputs[0].extension, "png");
}

#[test]
fn replicate_failures_carry_the_provider_error() {
    let (base, _server) = spawn_mock_replicate();
    let client = Replicate::with_base_url(&base)
        .with_polling(Duration::from_millis(10), Duration::from_secs(5));

    let err = client
        .generate(
            "key",
            &GenerationRequest {
                model: "acme/gen:deadbeef".to_owned(), // versioned → failed path
                inputs: serde_json::json!({"prompt": "x"}),
            },
        )
        .expect_err("generation fails");
    let message = err.to_string();
    assert!(
        message.contains("NSFW") || message.contains("failed"),
        "{message}"
    );
}

#[test]
fn recommended_models_come_from_collections() {
    let (base, _server) = spawn_mock_replicate();
    let client = Replicate::with_base_url(&base);
    let models = client.recommended_models("key").expect("models");
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].slug, "acme/dream-video");
    assert_eq!(models[0].version.as_deref(), Some("v123"));
    assert_eq!(models[0].kind, OutputKind::Video);
}
