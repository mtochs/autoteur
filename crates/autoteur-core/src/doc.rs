//! Parse + surgical-edit support. Every programmatic write to an Autoteur
//! file goes through `toml_edit` document editing so comments, key order,
//! formatting, and unknown keys survive byte-for-byte — a GUI gesture must
//! produce the same minimal diff an agent's text edit would.

use serde::de::DeserializeOwned;
use toml_edit::{DocumentMut, Item, Table, Value};

use crate::error::{Error, Result};

/// Parse `text` into a typed value plus an editable document. The document
/// is the write path; the typed value is the read path.
pub fn parse<T: DeserializeOwned>(text: &str) -> Result<(T, DocumentMut)> {
    let doc: DocumentMut = text.parse().map_err(|e| Error::Syntax(Box::new(e)))?;
    let data: T = toml_edit::de::from_str(text).map_err(|e| Error::Schema(Box::new(e)))?;
    Ok((data, doc))
}

/// True when `text` uses Windows line endings. Capture this at parse time
/// and pass it to [`serialize`] so an edit to a CRLF file stays CRLF —
/// toml_edit itself always emits bare `\n` for structural newlines, which
/// would turn one circled take into whole-file diff churn.
pub fn detect_crlf(text: &str) -> bool {
    text.contains("\r\n")
}

/// Render the document, restoring CRLF line endings when the source used
/// them. Mixed-EOL sources are normalized to their dominant convention.
pub fn serialize(doc: &DocumentMut, crlf: bool) -> String {
    let text = doc.to_string();
    if crlf {
        text.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        text
    }
}

/// Move the block at `from` to position `to` within a `[[key]]` array of
/// tables. Comments attached to a block travel with it.
pub fn move_block(doc: &mut DocumentMut, key: &str, from: usize, to: usize) -> Result<()> {
    // toml_edit serializes tables by their remembered document position,
    // not by array order — remap the existing slots onto the new order.
    // Tables added programmatically have no position yet; give them fresh
    // slots past the document-wide maximum so remapping never silently
    // skips (a skipped remap serializes in the ORIGINAL order).
    let max_position = max_table_position(doc);
    let aot = doc
        .get_mut(key)
        .and_then(Item::as_array_of_tables_mut)
        .ok_or_else(|| Error::Edit(format!("no [[{key}]] array in document")))?;
    let len = aot.len();
    if from >= len || to >= len {
        return Err(Error::Edit(format!(
            "block move {from} -> {to} out of range (len {len})"
        )));
    }
    let mut tables: Vec<Table> = std::mem::take(aot).into_iter().collect();
    let mut slots: Vec<isize> = tables.iter().filter_map(Table::position).collect();
    let mut next_fresh = max_position + 1;
    while slots.len() < tables.len() {
        slots.push(next_fresh);
        next_fresh += 1;
    }
    slots.sort_unstable();
    let moved = tables.remove(from);
    tables.insert(to, moved);
    for (table, slot) in tables.iter_mut().zip(&slots) {
        table.set_position(*slot);
    }
    for table in tables {
        aot.push(table);
    }
    Ok(())
}

pub(crate) fn max_table_position(doc: &DocumentMut) -> isize {
    fn walk(item: &Item, max: &mut isize) {
        match item {
            Item::Table(table) => {
                if let Some(p) = table.position() {
                    *max = (*max).max(p);
                }
                for (_, child) in table.iter() {
                    walk(child, max);
                }
            }
            Item::ArrayOfTables(aot) => {
                for table in aot.iter() {
                    if let Some(p) = table.position() {
                        *max = (*max).max(p);
                    }
                    for (_, child) in table.iter() {
                        walk(child, max);
                    }
                }
            }
            _ => {}
        }
    }
    let mut max = 0;
    for (_, item) in doc.iter() {
        walk(item, &mut max);
    }
    max
}

/// Set a scalar field on the block at `index` within `[[key]]`, preserving
/// any decoration (trailing comments) on an existing value.
pub fn set_block_field(
    doc: &mut DocumentMut,
    key: &str,
    index: usize,
    field: &str,
    value: Value,
) -> Result<()> {
    let table = block_mut(doc, key, index)?;
    set_table_field(table, field, value);
    Ok(())
}

/// Remove a field from the block at `index` within `[[key]]`. Returns true
/// when the field existed. Used e.g. to un-circle `selected_take`.
///
/// Comment lines physically above the removed key are parsed as its prefix
/// decor; they are re-attached to the following key so removing a field
/// deletes exactly the `key = value` line, like an agent's text edit would.
pub fn remove_block_field(
    doc: &mut DocumentMut,
    key: &str,
    index: usize,
    field: &str,
) -> Result<bool> {
    let table = block_mut(doc, key, index)?;
    let (removed, orphan) = remove_table_field(table, field);
    if let Some(prefix) = orphan {
        // The removed key was the last in its block: re-attach its comment
        // block to the next block's header instead.
        if let Some(next_table) = doc
            .get_mut(key)
            .and_then(Item::as_array_of_tables_mut)
            .and_then(|aot| aot.get_mut(index + 1))
        {
            let decor = next_table.decor_mut();
            let existing = decor
                .prefix()
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .to_owned();
            decor.set_prefix(format!("{prefix}{existing}"));
        }
    }
    Ok(removed)
}

/// Set a scalar field at document root (scene.toml-style files).
pub fn set_root_field(doc: &mut DocumentMut, field: &str, value: Value) {
    set_table_field(doc.as_table_mut(), field, value);
}

/// Remove a root-level field. Returns true when the field existed.
pub fn remove_root_field(doc: &mut DocumentMut, field: &str) -> bool {
    remove_table_field(doc.as_table_mut(), field).0
}

fn block_mut<'a>(doc: &'a mut DocumentMut, key: &str, index: usize) -> Result<&'a mut Table> {
    doc.get_mut(key)
        .and_then(Item::as_array_of_tables_mut)
        .and_then(|aot| aot.get_mut(index))
        .ok_or_else(|| Error::Edit(format!("no [[{key}]] block at index {index}")))
}

fn set_table_field(table: &mut Table, field: &str, mut value: Value) {
    match table.get_mut(field) {
        Some(Item::Value(existing)) => {
            *value.decor_mut() = existing.decor().clone();
            *existing = value;
        }
        Some(other) => *other = Item::Value(value),
        None => {
            table.insert(field, Item::Value(value));
        }
    }
}

/// Remove `field`, preserving its leading comment block: splice it onto the
/// next value key in the same table, or return it as an orphan for the
/// caller to re-attach when the removed key was last.
fn remove_table_field(table: &mut Table, field: &str) -> (bool, Option<String>) {
    if table.get(field).is_none() {
        return (false, None);
    }
    let saved_prefix: Option<String> = table.key(field).and_then(|k| {
        let prefix = k.leaf_decor().prefix()?.as_str()?;
        prefix.contains('#').then(|| prefix.to_owned())
    });
    let following_value_key: Option<String> = {
        let entries: Vec<(&str, bool)> = table
            .iter()
            .map(|(k, item)| (k, matches!(item, Item::Value(_))))
            .collect();
        entries
            .iter()
            .position(|(k, _)| *k == field)
            .and_then(|pos| entries.get(pos + 1))
            .filter(|(_, is_value)| *is_value)
            .map(|(k, _)| (*k).to_owned())
    };
    let removed = table.remove(field).is_some();
    if !removed {
        return (false, None);
    }
    let Some(prefix) = saved_prefix else {
        return (true, None);
    };
    if let Some(next) = following_value_key {
        if let Some(mut next_key) = table.key_mut(&next) {
            let decor = next_key.leaf_decor_mut();
            let existing = decor
                .prefix()
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .to_owned();
            decor.set_prefix(format!("{prefix}{existing}"));
            return (true, None);
        }
    }
    (true, Some(prefix))
}
