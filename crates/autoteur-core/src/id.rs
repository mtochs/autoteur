//! Identity types. One spelling per reference kind, all validating on parse:
//! slugs (`mara-chen`), per-scene shot letters (`a`..`z`, `aa`..), take ids
//! (`tk_` + 12 hex of the output's BLAKE3), shot refs (`scene-slug/shot-id`),
//! and cast entries with optional variant pins (`mara-chen:storm-gear`).

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::Error;

/// Kebab-case identifier: lowercase ASCII letters/digits separated by single
/// dashes. Used for beats, episodes, scenes, characters, and world entries.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Slug(String);

impl Slug {
    pub fn new(value: impl Into<String>) -> Result<Self, Error> {
        let value = value.into();
        let valid = !value.is_empty()
            && !value.starts_with('-')
            && !value.ends_with('-')
            && !value.contains("--")
            && value
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if valid {
            Ok(Self(value))
        } else {
            Err(Error::InvalidId {
                kind: "slug",
                value,
                expected: "kebab-case: lowercase letters/digits separated by single dashes",
            })
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Per-scene shot letter id: `a`..`z`, then `aa`, `ab`, ... Frozen at
/// creation and never reused within a scene.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ShotId(String);

impl ShotId {
    pub fn new(value: impl Into<String>) -> Result<Self, Error> {
        let value = value.into();
        let valid =
            !value.is_empty() && value.len() <= 4 && value.chars().all(|c| c.is_ascii_lowercase());
        if valid {
            Ok(Self(value))
        } else {
            Err(Error::InvalidId {
                kind: "shot id",
                value,
                expected: "1-4 lowercase letters (a, b, ... z, aa, ab, ...)",
            })
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Position in the bijective base-26 sequence: a=1, z=26, aa=27, ...
    pub fn index(&self) -> u32 {
        self.0
            .bytes()
            .fold(0u32, |acc, b| acc * 26 + u32::from(b - b'a') + 1)
    }

    fn from_index(mut n: u32) -> Self {
        let mut buf = Vec::new();
        while n > 0 {
            n -= 1;
            buf.push(b'a' + (n % 26) as u8);
            n /= 26;
        }
        buf.reverse();
        Self(String::from_utf8_lossy(&buf).into_owned())
    }

    /// The next unused shot id: one past the highest ever seen. Callers must
    /// pass ids from `shots.toml` AND from the takes manifest for the scene,
    /// so a deleted shot's letter is never reused.
    pub fn next_after<'a>(existing: impl IntoIterator<Item = &'a ShotId>) -> Self {
        let max = existing.into_iter().map(ShotId::index).max().unwrap_or(0);
        Self::from_index(max + 1)
    }
}

impl PartialOrd for ShotId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ShotId {
    fn cmp(&self, other: &Self) -> Ordering {
        self.index().cmp(&other.index())
    }
}

/// Content-addressed take id: `tk_` + first 12 lowercase hex chars of the
/// BLAKE3 hash of the take's primary output media.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TakeId(String);

impl TakeId {
    pub fn new(value: impl Into<String>) -> Result<Self, Error> {
        let value = value.into();
        let hex = value.strip_prefix("tk_").unwrap_or_default();
        let valid = hex.len() == 12
            && hex
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c));
        if valid {
            Ok(Self(value))
        } else {
            Err(Error::InvalidId {
                kind: "take id",
                value,
                expected: "\"tk_\" followed by 12 lowercase hex characters",
            })
        }
    }

    /// Derive the id from output media bytes.
    pub fn from_media_bytes(data: &[u8]) -> Self {
        let hex = blake3::hash(data).to_hex();
        Self(format!("tk_{}", &hex.as_str()[..12]))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Project-wide shot reference: `<scene-slug>/<shot-id>`, e.g.
/// `vault-breach/c`. The canonical key in the takes manifest and timeline.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ShotRef {
    pub scene: Slug,
    pub shot: ShotId,
}

impl ShotRef {
    pub fn new(scene: Slug, shot: ShotId) -> Self {
        Self { scene, shot }
    }
}

/// A shot's cast entry: a character slug with an optional pinned prompt
/// variant, written `mara-chen` or `mara-chen:storm-gear`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CastEntry {
    pub character: Slug,
    pub variant: Option<Slug>,
}

impl CastEntry {
    pub fn of(character: Slug) -> Self {
        Self {
            character,
            variant: None,
        }
    }
}

macro_rules! string_convertible {
    ($ty:ty) => {
        impl TryFrom<String> for $ty {
            type Error = Error;
            fn try_from(value: String) -> Result<Self, Error> {
                value.parse()
            }
        }

        impl From<$ty> for String {
            fn from(value: $ty) -> String {
                value.to_string()
            }
        }
    };
}

impl fmt::Display for Slug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Slug {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Error> {
        Self::new(s)
    }
}

impl fmt::Display for ShotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ShotId {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Error> {
        Self::new(s)
    }
}

impl fmt::Display for TakeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for TakeId {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Error> {
        Self::new(s)
    }
}

impl fmt::Display for ShotRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.scene, self.shot)
    }
}

impl FromStr for ShotRef {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Error> {
        let (scene, shot) = s.split_once('/').ok_or(Error::InvalidId {
            kind: "shot reference",
            value: s.to_owned(),
            expected: "\"<scene-slug>/<shot-id>\", e.g. \"vault-breach/c\"",
        })?;
        Ok(Self {
            scene: scene.parse()?,
            shot: shot.parse()?,
        })
    }
}

impl fmt::Display for CastEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.variant {
            Some(v) => write!(f, "{}:{}", self.character, v),
            None => write!(f, "{}", self.character),
        }
    }
}

impl FromStr for CastEntry {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Error> {
        match s.split_once(':') {
            Some((character, variant)) => Ok(Self {
                character: character.parse()?,
                variant: Some(variant.parse()?),
            }),
            None => Ok(Self {
                character: s.parse()?,
                variant: None,
            }),
        }
    }
}

string_convertible!(Slug);
string_convertible!(ShotId);
string_convertible!(TakeId);
string_convertible!(ShotRef);
string_convertible!(CastEntry);

#[cfg(test)]
mod tests {
    use super::*;

    fn slug(s: &str) -> Slug {
        Slug::new(s).expect("valid slug")
    }

    #[test]
    fn slug_accepts_kebab_case() {
        for ok in ["mara-chen", "e01", "a", "cold-open-heist", "0-1"] {
            assert!(Slug::new(ok).is_ok(), "{ok} should be valid");
        }
        for bad in ["", "Mara", "a--b", "-a", "a-", "a_b", "a b", "café"] {
            assert!(Slug::new(bad).is_err(), "{bad} should be invalid");
        }
    }

    #[test]
    fn shot_id_sequence_and_ordering() {
        let a = ShotId::new("a").expect("valid");
        let z = ShotId::new("z").expect("valid");
        let aa = ShotId::new("aa").expect("valid");
        assert_eq!(a.index(), 1);
        assert_eq!(z.index(), 26);
        assert_eq!(aa.index(), 27);
        assert!(a < z && z < aa);

        assert_eq!(ShotId::next_after([]).as_str(), "a");
        let existing = [a.clone(), ShotId::new("c").expect("valid")];
        assert_eq!(ShotId::next_after(existing.iter()).as_str(), "d");
        let existing = [z.clone()];
        assert_eq!(ShotId::next_after(existing.iter()).as_str(), "aa");
        let existing = [ShotId::new("az").expect("valid")];
        assert_eq!(ShotId::next_after(existing.iter()).as_str(), "ba");
    }

    #[test]
    fn shot_id_rejects_bad_forms() {
        for bad in ["", "A", "1", "abcde", "a-b"] {
            assert!(ShotId::new(bad).is_err(), "{bad} should be invalid");
        }
    }

    #[test]
    fn take_id_validates_and_derives() {
        assert!(TakeId::new("tk_3f9c2a8b41de").is_ok());
        for bad in [
            "3f9c2a8b41de",
            "tk_3F9C2A8B41DE",
            "tk_3f9c",
            "tk_zzzzzzzzzzzz",
        ] {
            assert!(TakeId::new(bad).is_err(), "{bad} should be invalid");
        }
        let id = TakeId::from_media_bytes(b"example media");
        assert_eq!(id.as_str().len(), 15);
        assert!(id.as_str().starts_with("tk_"));
        assert_eq!(id, TakeId::from_media_bytes(b"example media"));
    }

    #[test]
    fn shot_ref_round_trips() {
        let r: ShotRef = "vault-breach/c".parse().expect("valid ref");
        assert_eq!(r.scene, slug("vault-breach"));
        assert_eq!(r.to_string(), "vault-breach/c");
        assert!("vault-breach".parse::<ShotRef>().is_err());
        assert!("Vault/c".parse::<ShotRef>().is_err());
    }

    #[test]
    fn cast_entry_parses_variant_pin() {
        let plain: CastEntry = "mara-chen".parse().expect("valid");
        assert_eq!(plain.variant, None);
        let pinned: CastEntry = "mara-chen:storm-gear".parse().expect("valid");
        assert_eq!(pinned.variant, Some(slug("storm-gear")));
        assert_eq!(pinned.to_string(), "mara-chen:storm-gear");
        assert!("mara:Storm".parse::<CastEntry>().is_err());
    }
}
