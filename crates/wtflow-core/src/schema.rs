use anyhow::{anyhow, Context, Result};
use jsonschema::{Draft, JSONSchema};
use serde_json::Value;
use std::sync::OnceLock;
pub const FLOW: &str = include_str!("../../../schema/flow.schema.yaml");
pub const CONFIG: &str = include_str!("../../../schema/config.schema.yaml");
static FLOW_VALIDATOR: OnceLock<Result<JSONSchema, String>> = OnceLock::new();
static CONFIG_VALIDATOR: OnceLock<Result<JSONSchema, String>> = OnceLock::new();
pub fn value(config: bool) -> Result<Value> {
    serde_yaml_ng::from_str(if config { CONFIG } else { FLOW })
        .context("embedded schema:1: invalid YAML")
}
pub fn validate(value: &Value, config: bool) -> Result<()> {
    let cell = if config {
        &CONFIG_VALIDATOR
    } else {
        &FLOW_VALIDATOR
    };
    let compiled = cell.get_or_init(|| {
        let schema = self::value(config).map_err(|e| e.to_string())?;
        JSONSchema::options()
            .with_draft(Draft::Draft202012)
            .compile(&schema)
            .map_err(|e| e.to_string())
    });
    let validator = compiled
        .as_ref()
        .map_err(|e| anyhow!("embedded schema:1: {e}"))?;
    if let Err(errors) = validator.validate(value) {
        let mut messages: Vec<_> = errors
            .map(|e| format!("{}: {}", e.instance_path, e))
            .collect();
        messages.sort();
        return Err(anyhow!("E006 schema violation: {}", messages.join("; ")));
    }
    Ok(())
}
