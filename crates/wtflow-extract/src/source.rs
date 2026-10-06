use anyhow::{Context, Result};
use rayon::prelude::*;
use std::{collections::BTreeMap, path::Path};
use tree_sitter::{Node, Parser, Tree};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Ts,
    Py,
    Java,
}
impl Language {
    pub fn name(self) -> &'static str {
        match self {
            Self::Ts => "typescript",
            Self::Py => "python",
            Self::Java => "java",
        }
    }
    pub fn from_path(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()? {
            "ts" | "tsx" => Some(Self::Ts),
            "py" => Some(Self::Py),
            "java" => Some(Self::Java),
            _ => None,
        }
    }
}
pub struct SourceFile {
    pub path: String,
    pub text: String,
    pub tree: Tree,
    pub lang: Language,
}
impl SourceFile {
    pub fn parse(path: String, text: String) -> Result<Self> {
        let lang = Language::from_path(Path::new(&path)).context("unsupported source extension")?;
        let grammar = match lang {
            Language::Ts => {
                if path.ends_with(".tsx") {
                    tree_sitter_typescript::LANGUAGE_TSX.into()
                } else {
                    tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
                }
            }
            Language::Py => tree_sitter_python::LANGUAGE.into(),
            Language::Java => tree_sitter_java::LANGUAGE.into(),
        };
        let mut parser = Parser::new();
        parser
            .set_language(&grammar)
            .with_context(|| format!("{path}:1: grammar"))?;
        let tree = parser
            .parse(&text, None)
            .with_context(|| format!("{path}:1: parsing cancelled"))?;
        if tree.root_node().has_error() {
            let mut all = vec![];
            descendants(tree.root_node(), &mut all);
            if let Some(n) = all.iter().find(|n| n.is_error() || n.is_missing()) {
                anyhow::bail!(
                    "{path}:{}: syntax error ({})",
                    n.start_position().row + 1,
                    n.kind()
                );
            }
        }
        Ok(Self {
            path,
            text,
            tree,
            lang,
        })
    }
    pub fn text(&self, n: Node<'_>) -> &str {
        &self.text[n.byte_range()]
    }
    pub fn src(&self, n: Node<'_>) -> String {
        let start = n.start_position().row + 1;
        let end = n.end_position().row + 1;
        if start == end {
            format!("{}:{start}", self.path)
        } else {
            format!("{}:{start}-{end}", self.path)
        }
    }
}
pub fn children(n: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = n.walk();
    n.named_children(&mut cursor).collect()
}
pub fn descendants<'a>(n: Node<'a>, out: &mut Vec<Node<'a>>) {
    out.push(n);
    for child in children(n) {
        descendants(child, out);
    }
}
pub fn load(root: &Path) -> Result<BTreeMap<String, SourceFile>> {
    let mut paths = vec![];
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            !matches!(
                e.file_name().to_str(),
                Some(
                    "node_modules"
                        | ".git"
                        | ".wtflow"
                        | ".gradle"
                        | "build"
                        | "target"
                        | ".venv"
                        | "__pycache__"
                )
            )
        })
    {
        let entry = entry.with_context(|| format!("{}:1: scan", root.display()))?;
        if entry.file_type().is_file() && Language::from_path(entry.path()).is_some() {
            paths.push(entry.into_path());
        }
    }
    paths.sort();
    let files: Vec<Result<SourceFile>> = paths
        .par_iter()
        .map(|p| {
            let text =
                std::fs::read_to_string(p).with_context(|| format!("{}:1: read", p.display()))?;
            SourceFile::parse(
                p.strip_prefix(root)?.to_string_lossy().replace('\\', "/"),
                text,
            )
        })
        .collect();
    let mut result = BTreeMap::new();
    for file in files {
        let f = file?;
        result.insert(f.path.clone(), f);
    }
    Ok(result)
}
pub fn normalized(s: &str) -> String {
    // Collapse whitespace outside quoted literals, preserving literal contents.
    let mut out = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut space = false;
    for c in s.trim().trim_end_matches(';').chars() {
        if let Some(q) = quote {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
        } else if matches!(c, '\'' | '"' | '`') {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            quote = Some(c);
            out.push(c);
        } else if c.is_whitespace() {
            space = true;
        } else {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.push(c);
        }
    }
    out
}
