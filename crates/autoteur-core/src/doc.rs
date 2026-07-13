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

/// Move the block at `from` to position `to` within a `[[key]]` array of
/// tables. Comments attached to a block travel with it.
pub fn move_block(doc: &mut DocumentMut, key: &str, from: usize, to: usize) -> Result<()> {
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
    // toml_edit serializes tables by their remembered document position,
    // not by array order — remap the existing slots onto the new order.
    let mut slots: Vec<isize> = tables.iter().filter_map(Table::position).collect();
    slots.sort_unstable();
    let moved = tables.remove(from);
    tables.insert(to, moved);
    if slots.len() == tables.len() {
        for (table, slot) in tables.iter_mut().zip(&slots) {
            table.set_position(*slot);
        }
    }
    for table in tables {
        aot.push(table);
    }
    Ok(())
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
pub fn remove_block_field(
    doc: &mut DocumentMut,
    key: &str,
    index: usize,
    field: &str,
) -> Result<bool> {
    let table = block_mut(doc, key, index)?;
    Ok(table.remove(field).is_some())
}

/// Set a scalar field at document root (scene.toml-style files).
pub fn set_root_field(doc: &mut DocumentMut, field: &str, value: Value) {
    set_table_field(doc.as_table_mut(), field, value);
}

/// Remove a root-level field. Returns true when the field existed.
pub fn remove_root_field(doc: &mut DocumentMut, field: &str) -> bool {
    doc.as_table_mut().remove(field).is_some()
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
