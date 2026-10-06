use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub commit: String,
    pub dirty: bool,
    pub files: BTreeMap<String, String>,
    pub indexers: BTreeMap<String, String>,
}
pub fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
impl Metadata {
    pub fn load(root: &Path) -> Result<Option<Self>> {
        let file = root.join(".wtflow/index/meta.yaml");
        if !file.exists() {
            return Ok(None);
        }
        Ok(Some(
            serde_yaml_ng::from_str(&std::fs::read_to_string(&file)?)
                .with_context(|| format!("{}: invalid metadata", file.display()))?,
        ))
    }
    pub fn fresh(&self, file: &str, bytes: &[u8]) -> bool {
        self.files
            .get(file)
            .is_some_and(|expected| expected == &hash(bytes))
    }
    pub fn emit(&self) -> Result<String> {
        let mut out = format!(
            "commit: {}\ndirty: {}\n",
            wtflow_core::yaml::quote(&self.commit)?,
            self.dirty
        );
        for (key, map) in [("files", &self.files), ("indexers", &self.indexers)] {
            if map.is_empty() {
                out.push_str(&format!("{key}: {{}}\n"));
            } else {
                out.push_str(&format!("{key}:\n"));
                for (key, value) in map {
                    out.push_str(&format!(
                        "  {}: {}\n",
                        wtflow_core::yaml::quote(key)?,
                        wtflow_core::yaml::quote(value)?
                    ));
                }
            }
        }
        Ok(out)
    }
}
