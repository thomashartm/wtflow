use std::collections::BTreeSet;
use wtflow_core::{Flow, Node};

#[derive(Clone)]
pub struct Row {
    pub text: String,
    pub node: Option<Node>,
    pub parents: Vec<String>,
}
pub fn rows(flow: &Flow, expanded: &BTreeSet<String>) -> Vec<Row> {
    fn walk(
        nodes: &[Node],
        depth: usize,
        parents: &[String],
        expanded: &BTreeSet<String>,
        out: &mut Vec<Row>,
    ) {
        for node in nodes {
            let has_children = node.children().next().is_some();
            let marker = if !has_children {
                "·"
            } else if expanded.contains(&node.id) {
                "▾"
            } else {
                "▸"
            };
            out.push(Row {
                text: format!(
                    "{}{} {}  [{}]",
                    "  ".repeat(depth),
                    marker,
                    node.label.as_deref().unwrap_or(&node.code),
                    node.kind.as_str()
                ),
                node: Some(node.clone()),
                parents: parents.to_vec(),
            });
            if has_children && expanded.contains(&node.id) {
                let mut parents = parents.to_vec();
                parents.push(node.id.clone());
                let mut branches: Vec<(&str, &[Node])> =
                    vec![("Then", &node.then), ("Otherwise", &node.otherwise)];
                for case in &node.cases {
                    branches.push((&case.when, &case.steps));
                }
                branches.extend([
                    ("Default", node.default.as_slice()),
                    ("Body", node.body.as_slice()),
                    ("Catch", node.catch.as_slice()),
                    ("Finally", node.finally.as_slice()),
                ]);
                for (name, children) in branches {
                    if children.is_empty() {
                        continue;
                    }
                    out.push(Row {
                        text: format!("{}[{}]", "  ".repeat(depth + 1), name),
                        node: None,
                        parents: parents.clone(),
                    });
                    walk(children, depth + 2, &parents, expanded, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(&flow.steps, 0, &[], expanded, &mut out);
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn branch_names_and_ancestor_navigation_are_preserved() {
        let text = include_str!("../../../../testdata/golden/core.flow.yaml");
        let flow = wtflow_core::yaml::load(text, "fixture").unwrap();
        let mut nodes = Vec::new();
        wtflow_core::visit(&flow.steps, &mut nodes);
        let expanded = nodes.iter().map(|n| n.id.clone()).collect();
        let visible = rows(&flow, &expanded);
        assert_eq!(
            visible.iter().filter(|r| r.node.is_some()).count(),
            nodes.len()
        );
        for row in visible {
            for parent in row.parents {
                assert!(nodes.iter().any(|n| n.id == parent));
            }
        }
    }
}
