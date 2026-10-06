use crate::{
    functions::{self, Func},
    source::{self, SourceFile},
};
use anyhow::Result;
use regex::Regex;
use std::{
    collections::BTreeMap,
    path::{Component, Path},
};
use wtflow_resolve::{ByteRange, Def, HeuristicResolver};
fn normalize(path: &Path) -> String {
    let mut parts = vec![];
    for p in path.components() {
        match p {
            Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
            Component::ParentDir => {
                parts.pop();
            }
            _ => {}
        }
    }
    parts.join("/")
}
pub fn build(
    files: &BTreeMap<String, SourceFile>,
    funcs: &BTreeMap<String, Vec<Func>>,
    root: &Path,
) -> Result<HeuristicResolver> {
    let mut calls = BTreeMap::new();
    // Index once instead of scanning every function for every call site.
    let mut by_name: BTreeMap<&str, BTreeMap<&str, Vec<&Func>>> = BTreeMap::new();
    for func in funcs.values().flatten() {
        by_name
            .entry(&func.name)
            .or_default()
            .entry(&func.class)
            .or_default()
            .push(func);
    }
    // Type and import facts only guide target lookup; structural nodes always come from AST.
    let typed = Regex::new(r"(?:this\.|self\.)?(\w+)\s*:\s*([A-Za-z_]\w*)")?;
    let java_field = Regex::new(r"\b([A-Z]\w*)(?:<[^>]*>)?\s+(\w+)\s*[;=,)]")?;
    let assignment = Regex::new(r"self\.(\w+)\s*=\s*(\w+)")?;
    let imports = Regex::new(r#"import\s*\{([^}]+)\}\s*from\s*['"]([^'"]+)['"]"#)?;
    let py_import = Regex::new(r"from\s+([.\w]+)\s+import\s+(\w+)(?:\s+as\s+(\w+))?")?;
    let tsconfig: serde_json::Value = std::fs::read_to_string(root.join("tsconfig.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    for (path, file) in files {
        let mut types = BTreeMap::new();
        for cap in typed.captures_iter(&file.text) {
            types.insert(cap[1].to_owned(), cap[2].to_owned());
        }
        for cap in java_field.captures_iter(&file.text) {
            types.insert(cap[2].to_owned(), cap[1].to_owned());
        }
        for cap in assignment.captures_iter(&file.text) {
            if let Some(t) = types.get(&cap[2]).cloned() {
                types.insert(cap[1].into(), t);
            }
        }
        let mut imported: BTreeMap<String, String> = BTreeMap::new();
        for cap in imports.captures_iter(&file.text) {
            let mut spec = cap[2].to_owned();
            if !spec.starts_with('.') {
                if let Some(aliases) = tsconfig
                    .pointer("/compilerOptions/paths")
                    .and_then(|v| v.as_object())
                {
                    for (alias, targets) in aliases {
                        let prefix = alias.trim_end_matches('*');
                        if spec.starts_with(prefix) {
                            if let Some(target) = targets.get(0).and_then(|v| v.as_str()) {
                                spec = format!(
                                    "./{}{}",
                                    target.trim_end_matches('*'),
                                    spec.trim_start_matches(prefix)
                                );
                                break;
                            }
                        }
                    }
                }
            }
            let base = if cap[2].starts_with('.') {
                Path::new(path).parent().unwrap_or(Path::new(""))
            } else {
                Path::new("")
            };
            let destination = normalize(&base.join(spec));
            for item in cap[1].split(',') {
                let mut parts = item.trim().split(" as ");
                if let Some(name) = parts.next() {
                    let local = parts.next().unwrap_or(name);
                    imported.insert(local.into(), destination.clone());
                }
            }
        }
        for cap in py_import.captures_iter(&file.text) {
            let name = cap.get(3).map(|m| m.as_str()).unwrap_or(&cap[2]);
            imported.insert(
                name.into(),
                cap[1].trim_start_matches('.').replace('.', "/"),
            );
        }
        for call in functions::all_calls(file) {
            let Some(callee) = functions::callee(call) else {
                continue;
            };
            let text = functions::callee_text(file, call);
            let parts: Vec<_> = text.split('.').collect();
            let Some(method) = parts.last() else {
                continue;
            };
            let containing = funcs
                .get(path)
                .into_iter()
                .flatten()
                .filter(|f| f.range.start <= call.start_byte() && f.range.end >= call.end_byte())
                .min_by_key(|f| f.range.end - f.range.start);
            let class = if parts.len() == 1 {
                containing.map(|f| f.class.clone()).unwrap_or_default()
            } else {
                let receiver = parts[parts.len() - 2];
                if matches!(receiver, "this" | "self") {
                    containing.map(|f| f.class.clone()).unwrap_or_default()
                } else {
                    types
                        .get(receiver)
                        .cloned()
                        .unwrap_or_else(|| receiver.into())
                }
            };
            let named = by_name.get(method);
            let candidates: Vec<_> = named
                .and_then(|classes| classes.get(class.as_str()))
                .into_iter()
                .flatten()
                .copied()
                .chain(
                    named
                        .and_then(|classes| {
                            (parts.len() == 1 && !class.is_empty())
                                .then(|| classes.get(""))
                                .flatten()
                        })
                        .into_iter()
                        .flatten()
                        .copied(),
                )
                .filter(|f| {
                    if f.file == *path {
                        return true;
                    }
                    if let Some(dest) = imported.get(&class).or_else(|| imported.get(*method)) {
                        return f.file == format!("{dest}.ts")
                            || f.file == format!("{dest}.tsx")
                            || f.file == format!("{dest}.py");
                    }
                    if file.lang == source::Language::Java {
                        return Path::new(&f.file).parent() == Path::new(path).parent()
                            || file.text.contains(&format!(".{};", f.class));
                    }
                    false
                })
                .collect();
            if candidates.len() == 1 {
                let f = candidates[0];
                calls.insert(
                    (
                        path.clone(),
                        ByteRange {
                            start: callee.start_byte(),
                            end: callee.end_byte(),
                        },
                    ),
                    Def {
                        symbol: f.symbol(),
                        file: f.file.clone(),
                        range: f.range,
                    },
                );
            }
        }
    }
    Ok(HeuristicResolver::new(calls))
}
