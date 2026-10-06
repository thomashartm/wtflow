use crate::{Kind, Node};
use std::collections::{BTreeMap, BTreeSet};
const STOP: &str = "const let var await async this self return new if else for of in while true false null undefined None throw raise case switch not and or is typeof void final length continue break try catch do";
fn words(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut snake = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_uppercase()
            && i > 0
            && (chars[i - 1].is_lowercase()
                || (chars[i - 1].is_uppercase()
                    && chars.get(i + 1).is_some_and(|c| c.is_lowercase())))
        {
            snake.push('_');
        }
        if c.is_alphanumeric() {
            snake.extend(c.to_lowercase());
        } else {
            snake.push('_');
        }
    }
    snake
        .split('_')
        .filter(|w| {
            !w.is_empty()
                && !STOP.split_whitespace().any(|s| s.eq_ignore_ascii_case(w))
                && !w.chars().all(|c| c.is_numeric())
        })
        .map(str::to_owned)
        .collect()
}
fn base(n: &Node) -> String {
    let prefix = match n.kind {
        Kind::Call => "call",
        Kind::Emit => "emit",
        Kind::Group => "run",
        Kind::If => "if",
        Kind::Switch => "switch",
        Kind::ForEach => "each",
        Kind::While => "loop",
        Kind::Parallel => "par",
        Kind::Try => "try",
        Kind::Return => "return",
        Kind::Fail => "fail",
        Kind::Break => return "break".into(),
        Kind::Continue => return "continue".into(),
        Kind::Wait => "wait",
        Kind::Do => "",
    };
    let mut source = n.code.as_str();
    if n.kind == Kind::Try {
        if let Some(first) = n.body.first() {
            source = &first.code;
        }
    }
    if matches!(n.kind, Kind::Call | Kind::Emit | Kind::Group) {
        if let Some((callee, args)) = source.split_once('(') {
            let arg = args.trim_start();
            if let Some(quote @ ('\'' | '"' | '`')) = arg.chars().next() {
                if let Some(end) = arg[1..].find(quote) {
                    source = &arg[1..end + 1];
                }
            } else {
                let callee = callee.trim();
                let start = callee
                    .rmatch_indices('.')
                    .nth(1)
                    .map(|(i, _)| i + 1)
                    .unwrap_or(0);
                source = &callee[start..];
            }
        }
    }
    let mut parts = words(source);
    parts.truncate(3);
    let mut id = prefix.to_owned();
    for word in parts {
        if id.len() + word.len() + usize::from(!id.is_empty()) > 36 {
            break;
        }
        if !id.is_empty() {
            id.push('_');
        }
        id.push_str(&word);
    }
    if id.is_empty() {
        "step".into()
    } else {
        id
    }
}
pub fn assign(nodes: &mut [Node]) {
    fn one(n: &mut Node, counts: &mut BTreeMap<String, usize>, used: &mut BTreeSet<String>) {
        let stem = base(n);
        let count = counts.entry(stem.clone()).or_default();
        loop {
            *count += 1;
            n.id = if *count == 1 {
                stem.clone()
            } else {
                let suffix = format!("_{count}");
                let mut short = stem.clone();
                while short.len() + suffix.len() > 36 {
                    if let Some(i) = short.rfind('_') {
                        short.truncate(i);
                    } else {
                        short = "step".into();
                    }
                }
                format!("{short}{suffix}")
            };
            if used.insert(n.id.clone()) {
                break;
            }
        }
        for child in n.children_mut() {
            one(child, counts, used);
        }
    }
    let mut counts = BTreeMap::new();
    let mut used = BTreeSet::new();
    for n in nodes {
        one(n, &mut counts, &mut used);
    }
}
