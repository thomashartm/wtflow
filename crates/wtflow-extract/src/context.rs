//! Offline context packets for external labeling tools. This module makes no model calls.
use crate::Cx;
use anyhow::{Context, Result};
use serde::Serialize;
use wtflow_core::{Flow, Node};
#[derive(Debug, Serialize)]
pub struct CalleeContext {
    pub signature: String,
    pub documentation: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct Neighbors {
    pub previous: Option<String>,
    pub next: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct ContextPacket<'a> {
    pub node: &'a Node,
    pub path: String,
    pub callee: Option<CalleeContext>,
    pub ancestor_path: Vec<String>,
    pub neighbors: Neighbors,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub glossary: Option<serde_json::Value>,
}
fn callee(cx: &Cx, node: &Node) -> Option<CalleeContext> {
    let scip = cx.chain.scip.as_ref()?;
    let symbol = node.symbol.as_deref()?;
    if scip
        .definition(symbol)
        .is_some_and(|d| cx.chain.stale.contains(&d.file))
    {
        return None;
    }
    let info = scip.documentation(symbol)?;
    let mut signature = info.signature.clone();
    let mut documentation = vec![];
    for text in &info.documentation {
        let mut inside = false;
        let mut code = vec![];
        let mut prose = vec![];
        for line in text.lines() {
            if line.trim_start().starts_with("```") {
                inside = !inside;
                continue;
            }
            if inside {
                code.push(line);
            } else {
                prose.push(line);
            }
        }
        if signature.is_empty() && !code.is_empty() {
            signature = code.join("\n");
        }
        let prose = prose.join("\n").trim().to_owned();
        if !prose.is_empty() {
            documentation.push(prose);
        }
    }
    if signature.is_empty() {
        if let Some(def) = scip.definition(symbol) {
            if let Some(file) = cx.files.get(&def.file) {
                if let Some(func) = cx
                    .funcs
                    .get(&def.file)
                    .into_iter()
                    .flatten()
                    .filter(|f| f.range.start <= def.range.start && f.range.end >= def.range.end)
                    .min_by_key(|f| f.range.end - f.range.start)
                {
                    if let Some(ast) = crate::functions::node(file, func) {
                        if let Some(body) = ast.child_by_field_name("body") {
                            signature = crate::source::normalized(
                                &file.text[ast.start_byte()..body.start_byte()],
                            );
                        }
                    }
                }
            }
        }
    }
    Some(CalleeContext {
        signature,
        documentation,
    })
}
pub fn packets<'a>(cx: &Cx, flow: &'a Flow, all: bool) -> Result<Vec<ContextPacket<'a>>> {
    flow.verify_fingerprint()?;
    let path = cx.config.root.join("glossary.yaml");
    let glossary: Option<serde_json::Value> = if path.exists() {
        Some(
            serde_yaml_ng::from_str(&std::fs::read_to_string(&path)?)
                .with_context(|| format!("{}: invalid glossary YAML", path.display()))?,
        )
    } else {
        None
    };
    struct Builder<'c, 'n> {
        cx: &'c Cx,
        all: bool,
        glossary: Option<serde_json::Value>,
        out: Vec<ContextPacket<'n>>,
    }
    impl<'n> Builder<'_, 'n> {
        fn walk(&mut self, nodes: &'n [Node], path: &str, ancestors: &mut Vec<String>) {
            for (i, node) in nodes.iter().enumerate() {
                let path = format!("{path}[{i}]");
                if self.all || node.label.as_ref().map_or(true, String::is_empty) {
                    self.out.push(ContextPacket {
                        node,
                        path: path.clone(),
                        callee: callee(self.cx, node),
                        ancestor_path: ancestors.clone(),
                        neighbors: Neighbors {
                            previous: i.checked_sub(1).map(|i| nodes[i].id.clone()),
                            next: nodes.get(i + 1).map(|n| n.id.clone()),
                        },
                        glossary: self.glossary.clone(),
                    });
                }
                ancestors.push(node.id.clone());
                for (key, list) in [("then", &node.then), ("else", &node.otherwise)] {
                    self.walk(list, &format!("{path}.{key}"), ancestors);
                }
                for (j, case) in node.cases.iter().enumerate() {
                    self.walk(&case.steps, &format!("{path}.cases[{j}].steps"), ancestors);
                }
                for (key, list) in [
                    ("default", &node.default),
                    ("body", &node.body),
                    ("catch", &node.catch),
                    ("finally", &node.finally),
                ] {
                    self.walk(list, &format!("{path}.{key}"), ancestors);
                }
                ancestors.pop();
            }
        }
    }
    let mut builder = Builder {
        cx,
        all,
        glossary,
        out: vec![],
    };
    builder.walk(&flow.steps, "steps", &mut vec![]);
    Ok(builder.out)
}
