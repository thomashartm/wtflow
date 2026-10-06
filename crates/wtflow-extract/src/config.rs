use anyhow::{Context, Result};
use regex::Regex;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use wtflow_core::Kind;
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub modules: Vec<Module>,
    #[serde(default)]
    pub ignore: Vec<String>,
    #[serde(default = "default_tx")]
    pub tx_decorators: Vec<String>,
    #[serde(default = "yes")]
    pub collapse: bool,
    #[serde(default)]
    pub index: IndexConfig,
    #[serde(default)]
    pub rules: Vec<Rule>,
}
fn yes() -> bool {
    true
}
fn default_tx() -> Vec<String> {
    vec!["Transactional".into()]
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Module {
    pub path: String,
    pub owner: String,
}
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexConfig {
    pub typescript: Option<Indexer>,
    pub java: Option<Indexer>,
    pub python: Option<Indexer>,
}
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Indexer {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub args: Vec<String>,
    pub project_name: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    #[serde(rename = "match")]
    pub pattern: Option<String>,
    pub symbol: Option<String>,
    pub kind: Option<Kind>,
    pub boundary: Option<String>,
    pub topic: Option<String>,
    pub target: Option<String>,
    pub reads: Option<String>,
    pub writes: Option<String>,
    pub tx: Option<String>,
    #[serde(default)]
    pub inline_callback: bool,
}
pub struct CompiledRule {
    pub rule: Rule,
    pub pattern: Option<Regex>,
    pub symbol: Option<Regex>,
}
pub struct RepositoryConfig {
    pub root: PathBuf,
    pub config: Config,
    pub rules: Vec<CompiledRule>,
    pub ignore: Vec<Regex>,
}
impl RepositoryConfig {
    pub fn discover(entry: &Path) -> Result<Self> {
        let entry = entry
            .canonicalize()
            .with_context(|| format!("{}:1: cannot locate entry", entry.display()))?;
        let start = if entry.is_dir() {
            entry.as_path()
        } else {
            entry.parent().context("entry has no parent")?
        };
        let root = start
            .ancestors()
            .find(|p| p.join(".wtflow.yaml").is_file())
            .unwrap_or(start)
            .to_owned();
        Self::load(&root)
    }
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join(".wtflow.yaml");
        let mut config: Config = if path.exists() {
            let value: serde_json::Value =
                serde_yaml_ng::from_str(&std::fs::read_to_string(&path)?)
                    .with_context(|| format!("{}: invalid YAML", path.display()))?;
            wtflow_core::schema::validate(&value, true)
                .with_context(|| format!("{}:1", path.display()))?;
            serde_json::from_value(value)?
        } else {
            Config {
                collapse: true,
                tx_decorators: default_tx(),
                ..Config::default()
            }
        };
        let mut rules = vec![];
        for rule in std::mem::take(&mut config.rules) {
            let pattern = rule
                .pattern
                .as_deref()
                .map(Regex::new)
                .transpose()
                .with_context(|| format!("{}:1: invalid rule regex", path.display()))?;
            let symbol = rule
                .symbol
                .as_deref()
                .map(Regex::new)
                .transpose()
                .with_context(|| format!("{}:1: invalid symbol regex", path.display()))?;
            rules.push(CompiledRule {
                rule,
                pattern,
                symbol,
            });
        }
        let mut ignore = vec![Regex::new(
            r"^(?:(?:this\.|self\.)?(?:logger|log|logging)\.|console\.|Log\.|System\.out\.|print\()",
        )?];
        for pattern in &config.ignore {
            ignore.push(
                Regex::new(pattern)
                    .with_context(|| format!("{}:1: invalid ignore regex", path.display()))?,
            );
        }
        Ok(Self {
            root: root.to_owned(),
            config,
            rules,
            ignore,
        })
    }
    pub fn owner(&self, path: &str) -> String {
        self.config
            .modules
            .iter()
            .filter(|m| {
                m.path == "."
                    || path == m.path
                    || path.starts_with(&format!("{}/", m.path.trim_end_matches('/')))
            })
            .max_by_key(|m| m.path.len())
            .map(|m| m.owner.clone())
            .unwrap_or_default()
    }
}
