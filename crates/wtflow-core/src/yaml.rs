//! Handwritten canonical YAML. serde_yaml_ng is used only to parse.
use crate::{Flow, Kind, Node};
use anyhow::{Context, Result};

pub fn load(text: &str, file: &str) -> Result<Flow> {
    let value: serde_json::Value =
        serde_yaml_ng::from_str(text).with_context(|| format!("{file}: E000 parse error"))?;
    crate::schema::validate(&value, false).with_context(|| format!("{file}:1"))?;
    serde_json::from_value(value).with_context(|| format!("{file}:1: E000 invalid flow"))
}
pub fn quote(s: &str) -> Result<String> {
    let first = s.chars().next();
    let reserved = matches!(
        s.to_ascii_lowercase().as_str(),
        "true"
            | "false"
            | "null"
            | "~"
            | "yes"
            | "no"
            | "on"
            | "off"
            | ".nan"
            | ".inf"
            | "+.inf"
            | "-.inf"
    );
    let typed = !matches!(
        serde_yaml_ng::from_str::<serde_yaml_ng::Value>(s),
        Ok(serde_yaml_ng::Value::String(_))
    );
    let date = s.len() >= 10
        && s.as_bytes().get(4) == Some(&b'-')
        && s.as_bytes().get(7) == Some(&b'-')
        && s[..4].bytes().all(|c| c.is_ascii_digit());
    let indicator = first.is_some_and(|c| "-?:,[]{}#&*!|>'\"%@`".contains(c));
    if s.is_empty()
        || s.trim() != s
        || indicator
        || reserved
        || typed
        || date
        || s.contains(": ")
        || s.contains(" #")
        || s.chars().any(char::is_control)
    {
        Ok(serde_json::to_string(s)?)
    } else {
        Ok(s.to_owned())
    }
}
struct Emitter {
    out: String,
}
impl Emitter {
    fn line(&mut self, indent: usize, text: &str) {
        self.out.push_str(&" ".repeat(indent));
        self.out.push_str(text);
        self.out.push('\n');
    }
    fn field(&mut self, indent: usize, key: &str, value: &str) -> Result<()> {
        self.line(indent, &format!("{key}: {}", quote(value)?));
        Ok(())
    }
    fn list(&mut self, indent: usize, key: &str, values: &[String], flow: bool) -> Result<()> {
        if flow {
            let parts: Result<Vec<_>> = values
                .iter()
                .map(|s| {
                    // YAML flow scalars have additional delimiters.
                    if s.contains([',', '[', ']', '{', '}']) {
                        Ok(serde_json::to_string(s)?)
                    } else {
                        quote(s)
                    }
                })
                .collect();
            self.line(indent, &format!("{key}: [{}]", parts?.join(", ")));
        } else if !values.is_empty() {
            self.line(indent, &format!("{key}:"));
            for v in values {
                self.line(indent + 2, &format!("- {}", quote(v)?));
            }
        }
        Ok(())
    }
    fn nodes(&mut self, indent: usize, key: &str, nodes: &[Node], required: bool) -> Result<()> {
        if nodes.is_empty() {
            if required {
                self.line(indent, &format!("{key}: []"));
            }
            return Ok(());
        }
        self.line(indent, &format!("{key}:"));
        for n in nodes {
            self.node(indent + 2, n)?;
        }
        Ok(())
    }
    fn node(&mut self, indent: usize, n: &Node) -> Result<()> {
        self.line(indent, &format!("- id: {}", quote(&n.id)?));
        let d = indent + 2;
        self.field(d, "kind", n.kind.as_str())?;
        if let Some(label) = &n.label {
            self.field(d, "label", label)?;
        }
        if n.code.contains('\n') {
            anyhow::ensure!(
                n.kind == Kind::Do && !n.code.ends_with('\n') && !n.code.contains('\r'),
                "{}: multiline code requires a collapsed do without trailing newline",
                n.src
            );
            self.line(d, "code: |-");
            for line in n.code.split('\n') {
                self.line(d + 2, line);
            }
        } else if !n.code.is_empty() {
            self.field(d, "code", &n.code)?;
        }
        if !n.src.is_empty() {
            self.field(d, "src", &n.src)?;
        }
        for (k, v) in [
            ("target", &n.target),
            ("symbol", &n.symbol),
            ("boundary", &n.boundary),
            ("topic", &n.topic),
            ("tx", &n.tx),
        ] {
            if let Some(v) = v {
                self.field(d, k, v)?;
            }
        }
        self.list(d, "reads", &n.reads, false)?;
        self.list(d, "writes", &n.writes, false)?;
        self.nodes(d, "then", &n.then, false)?;
        self.nodes(d, "else", &n.otherwise, false)?;
        if !n.cases.is_empty() {
            self.line(d, "cases:");
            for c in &n.cases {
                self.line(d + 2, &format!("- when: {}", quote(&c.when)?));
                if let Some(f) = c.fallthrough {
                    self.line(d + 4, &format!("fallthrough: {f}"));
                }
                self.nodes(d + 4, "steps", &c.steps, true)?;
            }
        }
        for (k, v) in [
            ("default", &n.default),
            ("body", &n.body),
            ("catch", &n.catch),
            ("finally", &n.finally),
        ] {
            self.nodes(d, k, v, false)?;
        }
        Ok(())
    }
}
pub fn emit(flow: &Flow) -> Result<String> {
    crate::schema::validate(&serde_json::to_value(flow)?, false)?;
    let mut e = Emitter { out: String::from("# yaml-language-server: $schema=../../schema/flow.schema.yaml\n# Generated by wtflow. Only `label` fields may be edited (use `wtflow label`).\n") };
    e.field(0, "flow", &flow.flow)?;
    e.line(0, "version: 1");
    if !flow.owner.is_empty() {
        e.field(0, "owner", &flow.owner)?;
    }
    if !flow.trigger.is_empty() {
        e.field(0, "trigger", &flow.trigger)?;
    }
    e.line(0, "entry:");
    e.field(2, "lang", &flow.entry.lang)?;
    e.field(2, "file", &flow.entry.file)?;
    e.field(2, "symbol", &flow.entry.symbol)?;
    e.line(2, &format!("depth: {}", flow.entry.depth));
    e.field(0, "resolution", flow.resolution.as_str())?;
    e.field(0, "fingerprint", &flow.fingerprint)?;
    if !flow.inputs.is_empty() {
        e.line(0, "inputs:");
        for input in &flow.inputs {
            e.line(2, &format!("- name: {}", quote(&input.name)?));
            e.field(4, "type", &input.ty)?;
        }
    }
    if !flow.output.is_empty() {
        e.field(0, "output", &flow.output)?;
    }
    e.line(0, "state:");
    e.list(2, "reads", &flow.state.reads, true)?;
    e.list(2, "writes", &flow.state.writes, true)?;
    e.list(0, "boundaries", &flow.boundaries, true)?;
    e.nodes(0, "steps", &flow.steps, true)?;
    Ok(e.out)
}
