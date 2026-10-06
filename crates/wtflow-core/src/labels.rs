use crate::{Flow, Node};
use anyhow::Result;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
pub fn apply(flow: &mut Flow, labels: &BTreeMap<String, String>) -> Result<()> {
    flow.verify_fingerprint()?;
    let mut nodes = vec![];
    crate::visit(&flow.steps, &mut nodes);
    let ids: BTreeSet<_> = nodes.iter().map(|n| n.id.as_str()).collect();
    for id in labels.keys() {
        anyhow::ensure!(
            ids.contains(id.as_str()),
            "unknown id {id}; no labels applied"
        );
    }
    fn patch(n: &mut Node, labels: &BTreeMap<String, String>) {
        if let Some(label) = labels.get(&n.id) {
            n.label = Some(label.clone());
        }
        for c in n.children_mut() {
            patch(c, labels);
        }
    }
    for n in &mut flow.steps {
        patch(n, labels);
    }
    flow.refresh_fingerprint()
}
pub fn carry(old: &Flow, new: &mut Flow) -> Result<usize> {
    old.verify_fingerprint()?;
    let mut nodes = vec![];
    crate::visit(&old.steps, &mut nodes);
    let labels: BTreeMap<_, _> = nodes
        .into_iter()
        .filter_map(|n| {
            n.label
                .as_ref()
                .map(|l| ((n.id.clone(), n.code.clone()), l.clone()))
        })
        .collect();
    fn patch(n: &mut Node, labels: &BTreeMap<(String, String), String>) -> usize {
        let mut count = 0;
        if let Some(label) = labels.get(&(n.id.clone(), n.code.clone())) {
            n.label = Some(label.clone());
            count += 1;
        }
        for c in n.children_mut() {
            count += patch(c, labels);
        }
        count
    }
    let count = new.steps.iter_mut().map(|n| patch(n, &labels)).sum();
    new.refresh_fingerprint()?;
    Ok(count)
}
#[derive(Serialize)]
pub struct Todo<'a> {
    pub id: &'a str,
    pub kind: crate::Kind,
    pub path: String,
    pub code: &'a str,
    pub target: &'a Option<String>,
    pub symbol: &'a Option<String>,
    pub src: &'a str,
}
pub fn todo(flow: &Flow, all: bool) -> Vec<Todo<'_>> {
    fn walk<'a>(nodes: &'a [Node], path: &str, all: bool, out: &mut Vec<Todo<'a>>) {
        for (i, n) in nodes.iter().enumerate() {
            let path = format!("{path}[{i}]");
            if all || n.label.as_ref().map_or(true, String::is_empty) {
                out.push(Todo {
                    id: &n.id,
                    kind: n.kind,
                    path: path.clone(),
                    code: &n.code,
                    target: &n.target,
                    symbol: &n.symbol,
                    src: &n.src,
                });
            }
            for (key, list) in [("then", &n.then), ("else", &n.otherwise)] {
                walk(list, &format!("{path}.{key}"), all, out);
            }
            for (i, c) in n.cases.iter().enumerate() {
                walk(&c.steps, &format!("{path}.cases[{i}].steps"), all, out);
            }
            for (key, list) in [
                ("default", &n.default),
                ("body", &n.body),
                ("catch", &n.catch),
                ("finally", &n.finally),
            ] {
                walk(list, &format!("{path}.{key}"), all, out);
            }
        }
    }
    let mut out = vec![];
    walk(&flow.steps, "steps", all, &mut out);
    out
}
