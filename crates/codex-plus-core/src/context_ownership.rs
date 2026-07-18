use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const CONTEXT_OWNERSHIP_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextEntryIdentity {
    pub kind: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnedContextEntry {
    pub identity: ContextEntryIdentity,
    pub body_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextOwnershipManifest {
    pub version: u32,
    pub entries: Vec<OwnedContextEntry>,
}

impl Default for ContextOwnershipManifest {
    fn default() -> Self {
        Self {
            version: CONTEXT_OWNERSHIP_VERSION,
            entries: Vec::new(),
        }
    }
}

impl ContextOwnershipManifest {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.version != CONTEXT_OWNERSHIP_VERSION {
            anyhow::bail!("unsupported context ownership manifest version");
        }

        let mut identities = HashSet::new();
        for entry in &self.entries {
            if !matches!(entry.identity.kind.as_str(), "mcp" | "skill" | "plugin") {
                anyhow::bail!("invalid context ownership kind");
            }
            if entry.identity.id.trim().is_empty() {
                anyhow::bail!("context ownership id cannot be empty");
            }
            if !is_lower_hex_sha256(&entry.body_sha256) {
                anyhow::bail!("invalid context ownership body hash");
            }
            if !identities.insert(entry.identity.clone()) {
                anyhow::bail!("duplicate context ownership identity");
            }
        }
        Ok(())
    }

    pub fn revision(&self) -> ContextOwnershipRevision {
        let bytes = canonical_manifest_bytes(self);
        ContextOwnershipRevision(hex_sha256(&bytes))
    }

    pub fn validated_json_bytes(&self) -> anyhow::Result<Vec<u8>> {
        self.validate()?;
        let mut canonical = self.clone();
        canonical
            .entries
            .sort_by(|left, right| left.identity.cmp(&right.identity));
        let mut bytes = serde_json::to_vec_pretty(&canonical)?;
        bytes.push(b'\n');
        Ok(bytes)
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ContextOwnershipRevision(String);

impl ContextOwnershipRevision {
    pub fn parse(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        is_lower_hex_sha256(&value).then_some(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ContextOwnershipRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("ContextOwnershipRevision")
            .field(&self.0)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContextSyncDiff {
    pub added: Vec<ContextEntryIdentity>,
    pub updated: Vec<ContextEntryIdentity>,
    pub removed: Vec<ContextEntryIdentity>,
    pub unchanged: Vec<ContextEntryIdentity>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ContextSyncPlan {
    pub updated_live_config: String,
    pub next_manifest: ContextOwnershipManifest,
    pub diff: ContextSyncDiff,
}

impl fmt::Debug for ContextSyncPlan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextSyncPlan")
            .field("added", &self.diff.added.len())
            .field("updated", &self.diff.updated.len())
            .field("removed", &self.diff.removed.len())
            .field("unchanged", &self.diff.unchanged.len())
            .finish_non_exhaustive()
    }
}

pub fn load_context_ownership_at(path: &Path) -> anyhow::Result<ContextOwnershipManifest> {
    if !path.exists() {
        return Ok(ContextOwnershipManifest::default());
    }
    let manifest: ContextOwnershipManifest = serde_json::from_slice(&fs::read(path)?)?;
    manifest.validate()?;
    Ok(manifest)
}

pub fn save_context_ownership_at(
    path: &Path,
    manifest: &ContextOwnershipManifest,
) -> anyhow::Result<()> {
    let bytes = manifest.validated_json_bytes()?;
    crate::settings::atomic_write(path, &bytes)
}

pub(crate) fn normalized_body_sha256(body: &str) -> String {
    hex_sha256(body.as_bytes())
}

fn canonical_manifest_bytes(manifest: &ContextOwnershipManifest) -> Vec<u8> {
    let mut canonical = manifest.clone();
    canonical
        .entries
        .sort_by(|left, right| left.identity.cmp(&right.identity));
    serde_json::to_vec(&canonical).expect("context ownership manifest serialization cannot fail")
}

fn hex_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn is_lower_hex_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
