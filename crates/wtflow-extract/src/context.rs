//! Offline context packets for external labeling tools. This module makes no model calls.
use crate::Cx;
use anyhow::{Context, Result};
use serde::Serialize;
pub use wtflow_core::context::CallDetails as CalleeContext;
use wtflow_core::{Flow, Node};
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
fn definition<'a>(cx: &'a Cx, node: &Node) -> Option<&'a crate::Func> {
    let indexed = node
        .symbol
        .as_deref()
        .and_then(|symbol| cx.chain.scip.as_ref()?.definition(symbol))
        .filter(|def| !cx.chain.stale.contains(&def.file));
    let (file, range) = if let Some(def) = indexed {
        (def.file.clone(), def.range)
    } else {
        // Use the call's exact source location for heuristic targets; names alone
        // are ambiguous when different modules have the same class or method.
        let (path, _) = node.src.rsplit_once(':')?;
        let source = cx.files.get(path)?;
        let call = crate::functions::all_calls(source)
            .into_iter()
            .find(|call| source.src(*call) == node.src && source.normalized(*call) == node.code)?;
        let callee = crate::functions::callee(call)?;
        match cx.resolver().resolve(
            &path.to_owned(),
            wtflow_resolve::ByteRange {
                start: callee.start_byte(),
                end: callee.end_byte(),
            },
        ) {
            wtflow_resolve::Resolution::Def { file, range, .. } => (file, range),
            _ => return None,
        }
    };
    cx.funcs
        .get(&file)?
        .iter()
        .filter(|f| f.range.start <= range.start && f.range.end >= range.end)
        .min_by_key(|f| f.range.end - f.range.start)
}

fn source_comment(file: &crate::SourceFile, func: &crate::Func) -> Vec<String> {
    let Some(ast) = crate::functions::node(file, func) else {
        return vec![];
    };
    if file.lang == crate::source::Language::Py {
        if let Some(body) = ast.child_by_field_name("body") {
            if let Some(first) = crate::source::children(body).first().copied() {
                let string = if first.kind() == "string" {
                    Some(first)
                } else if first.kind() == "expression_statement" {
                    crate::source::children(first)
                        .into_iter()
                        .find(|n| n.kind() == "string")
                } else {
                    None
                };
                if let Some(string) = string {
                    let text = file.text(string).trim_start_matches(['r', 'u', 'R', 'U']);
                    let text = text.trim_matches(['\'', '"']).trim();
                    if !text.is_empty() {
                        return vec![text.into()];
                    }
                }
            }
        }
    }
    let declaration = ast
        .parent()
        .filter(|p| matches!(p.kind(), "export_statement" | "decorated_definition"))
        .unwrap_or(ast);
    let mut previous = declaration.prev_named_sibling();
    let mut start_row = declaration.start_position().row;
    let mut comments = Vec::new();
    while let Some(n) = previous {
        if n.kind() == "decorator" {
            start_row = n.start_position().row;
            previous = n.prev_named_sibling();
            continue;
        }
        if !matches!(n.kind(), "comment" | "block_comment" | "line_comment")
            || n.end_position().row + 1 < start_row
        {
            break;
        }
        // A trailing comment on the previous declaration does not document this one.
        if n.prev_named_sibling()
            .is_some_and(|p| p.end_position().row == n.start_position().row)
        {
            break;
        }
        let text = file.text(n).trim();
        let text = text
            .strip_prefix("/**")
            .or_else(|| text.strip_prefix("/*"))
            .unwrap_or(text)
            .trim_end_matches("*/");
        let text = text
            .lines()
            .map(|line| {
                line.trim()
                    .trim_start_matches("//")
                    .trim_start_matches('*')
                    .trim()
            })
            .collect::<Vec<_>>()
            .join("\n");
        if !text.trim().is_empty() {
            comments.push(text.trim().to_owned());
        }
        start_row = n.start_position().row;
        previous = n.prev_named_sibling();
    }
    comments.reverse();
    comments
}

fn return_type(signature: &str) -> String {
    let mut depth = 0usize;
    for (at, ch) in signature.char_indices() {
        if ch == '(' {
            depth += 1;
        }
        if ch == ')' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                let tail = signature[at + 1..].trim();
                if let Some(ty) = tail.strip_prefix(':').or_else(|| tail.strip_prefix("->")) {
                    return ty.trim().trim_end_matches([';', ':']).trim().into();
                }
            }
        }
    }
    String::new()
}

pub fn callee(cx: &Cx, node: &Node) -> Option<CalleeContext> {
    let mut result = CalleeContext::default();
    let indexed = node.symbol.as_deref().and_then(|symbol| {
        let scip = cx.chain.scip.as_ref()?;
        if scip
            .definition(symbol)
            .is_some_and(|d| cx.chain.stale.contains(&d.file))
        {
            return None;
        }
        scip.documentation(symbol)
    });
    if let Some(info) = indexed {
        result.signature = info.signature.clone();
        for text in &info.documentation {
            let mut inside = false;
            let mut code = Vec::new();
            let mut prose = Vec::new();
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
            if result.signature.is_empty() && !code.is_empty() {
                result.signature = code.join("\n");
            }
            let prose = prose.join("\n").trim().to_owned();
            if !prose.is_empty() {
                result.documentation.push(prose);
            }
        }
    }
    if let Some(func) = definition(cx, node) {
        if let Some(file) = cx.files.get(&func.file) {
            if let Some(ast) = crate::functions::node(file, func) {
                result.definition = file.src(ast);
                result.return_type = func.output.clone();
                if result.signature.is_empty() {
                    if let Some(body) = ast.child_by_field_name("body") {
                        result.signature = crate::source::normalized(
                            &file.text[ast.start_byte()..body.start_byte()],
                        );
                    }
                }
                let comments = source_comment(file, func);
                if !comments.is_empty() {
                    result.documentation = comments;
                }
            }
        }
    }
    if result.return_type.is_empty() {
        result.return_type = return_type(&result.signature);
    }
    (!result.signature.is_empty()
        || !result.documentation.is_empty()
        || !result.definition.is_empty())
    .then_some(result)
}

pub fn flow_context(cx: &Cx, flow: &Flow) -> wtflow_core::context::FlowContext {
    let mut nodes = Vec::new();
    wtflow_core::visit(&flow.steps, &mut nodes);
    wtflow_core::context::FlowContext {
        fingerprint: flow.fingerprint.clone(),
        calls: nodes
            .into_iter()
            .filter(|n| {
                matches!(
                    n.kind,
                    wtflow_core::Kind::Call
                        | wtflow_core::Kind::Group
                        | wtflow_core::Kind::Emit
                        | wtflow_core::Kind::Wait
                )
            })
            .filter_map(|n| callee(cx, n).map(|info| (n.id.clone(), info)))
            .collect(),
    }
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
