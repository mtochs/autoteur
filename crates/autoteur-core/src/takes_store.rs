//! Content-addressed take storage + manifest appends. Media lives under
//! gitignored `takes/<2hex>/<hash>.<ext>`; the committed manifest records
//! every generation's full parameters so any take can be re-printed from
//! the negative. Takes are immutable: bit-identical regenerations dedupe
//! onto the existing take instead of duplicating it.

use std::fs;
use std::path::Path;

use serde::Serialize;
use toml_edit::Item;

use crate::error::{Error, Result};
use crate::id::TakeId;
use crate::provider::GeneratedOutput;
use crate::schema::takes::{TakeOutput, TakeRecord, TakesManifest};
use crate::{atomic, doc, project};

const T_TAKES: &str = include_str!("../templates/takes.manifest.toml");

#[derive(Debug, Clone)]
pub struct StoredTake {
    pub id: TakeId,
    pub outputs: Vec<TakeOutput>,
}

/// Write outputs into the content-addressed store. The take id derives
/// from the primary (first) output's bytes.
pub fn store_outputs(root: &Path, outputs: &[GeneratedOutput]) -> Result<StoredTake> {
    let primary = outputs
        .first()
        .ok_or_else(|| Error::Generation("the provider returned no output files".to_owned()))?;
    let id = TakeId::from_media_bytes(&primary.bytes);

    let mut stored = Vec::new();
    for output in outputs {
        let hash = blake3::hash(&output.bytes).to_hex().to_string();
        let rel = format!("takes/{}/{hash}.{}", &hash[..2], output.extension);
        let abs = root.join(&rel);
        if !abs.exists() {
            if let Some(parent) = abs.parent() {
                fs::create_dir_all(parent).map_err(|e| Error::Io {
                    path: parent.to_owned(),
                    source: e,
                })?;
            }
            fs::write(&abs, &output.bytes).map_err(|e| Error::Io {
                path: abs.clone(),
                source: e,
            })?;
        }
        stored.push(TakeOutput {
            hash,
            kind: Some(output.kind.as_str().to_owned()),
            path: Some(rel),
            duration_s: None,
        });
    }
    Ok(StoredTake {
        id,
        outputs: stored,
    })
}

/// Append a record to takes.manifest.toml, preserving the file's comments.
/// Returns false when the take id already exists — a bit-identical
/// regeneration dedupes onto the existing take.
pub fn append_take(root: &Path, record: &TakeRecord) -> Result<bool> {
    let path = root.join("takes.manifest.toml");
    let text = if path.exists() {
        project::read_text(&path)?
    } else {
        T_TAKES.to_owned()
    };
    let (manifest, mut document): (TakesManifest, toml_edit::DocumentMut) = doc::parse(&text)?;
    if manifest.takes.iter().any(|t| t.id == record.id) {
        return Ok(false);
    }

    // Render the record as standalone TOML (pretty form gives real
    // [[takes]] blocks), then move its block into the live document so
    // existing comments/entries stay untouched.
    #[derive(Serialize)]
    struct Wrapper<'a> {
        takes: [&'a TakeRecord; 1],
    }
    let rendered_text = toml::to_string_pretty(&Wrapper { takes: [record] })
        .map_err(|e| Error::Generation(format!("couldn't serialize take record: {e}")))?;
    let rendered: toml_edit::DocumentMut = rendered_text
        .parse()
        .map_err(|e| Error::Generation(format!("take record round-trip failed: {e}")))?;
    let table = rendered
        .get("takes")
        .and_then(Item::as_array_of_tables)
        .and_then(|aot| aot.iter().next())
        .cloned()
        .ok_or_else(|| Error::Generation("take record serialized to nothing".to_owned()))?;

    let next_position = doc::max_table_position(&document) + 1;
    if document.get("takes").is_none() {
        document.insert(
            "takes",
            Item::ArrayOfTables(toml_edit::ArrayOfTables::new()),
        );
    }
    let aot = document
        .get_mut("takes")
        .and_then(Item::as_array_of_tables_mut)
        .ok_or_else(|| Error::Edit("takes.manifest.toml has a non-array `takes` key".to_owned()))?;
    let mut table = table;
    table.set_position(next_position);
    aot.push(table);

    let crlf = doc::detect_crlf(&text);
    atomic::write_atomic(&path, doc::serialize(&document, crlf).as_bytes())?;
    Ok(true)
}
