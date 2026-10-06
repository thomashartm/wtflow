use anyhow::{Context, Result};
use rayon::prelude::*;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
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
        let mut file = Self::parse_unchecked(path, text)?;
        if file.lang == Language::Ts && file.tree.root_node().has_error() {
            if let Some(input) = typescript_compatibility_input(&file) {
                file.tree = Self::parse_unchecked(file.path.clone(), input)?.tree;
            }
        }
        let tree = &file.tree;
        let path = &file.path;
        if tree.root_node().has_error() {
            fn first_error(n: Node<'_>) -> Option<Node<'_>> {
                if n.is_error() || n.is_missing() {
                    return Some(n);
                }
                let mut cursor = n.walk();
                let found = n.children(&mut cursor).find_map(first_error);
                found
            }
            if let Some(n) = first_error(tree.root_node()) {
                anyhow::bail!(
                    "{path}:{}: syntax error ({})",
                    n.start_position().row + 1,
                    n.kind()
                );
            }
            anyhow::bail!("{path}:1: syntax error");
        }
        Ok(file)
    }
    /// Retain parser errors for the debug-ast development command.
    pub fn parse_unchecked(path: String, text: String) -> Result<Self> {
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
    /// Remove AST comment nodes before flattening code. Quotes in comments must
    /// not affect literal tracking, and // must not swallow the following code.
    pub fn normalized(&self, n: Node<'_>) -> String {
        fn append(file: &SourceFile, n: Node<'_>, offset: &mut usize, out: &mut String) {
            if matches!(n.kind(), "comment" | "line_comment" | "block_comment") {
                out.push_str(&file.text[*offset..n.start_byte()]);
                out.push(' ');
                *offset = n.end_byte();
            } else {
                for child in children(n) {
                    append(file, child, offset, out);
                }
            }
        }
        let mut code = String::new();
        let mut offset = n.start_byte();
        append(self, n, &mut offset, &mut code);
        code.push_str(&self.text[offset..n.end_byte()]);
        normalized(&code)
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

/// Work around three tree-sitter-typescript 0.23 grammar gaps, only where the
/// original AST establishes the context. Reparse the projection strictly; never
/// accept arbitrary ERROR nodes. Source text and every byte/line offset stay intact.
fn typescript_compatibility_input(file: &SourceFile) -> Option<String> {
    let mut nodes = Vec::new();
    descendants(file.tree.root_node(), &mut nodes);
    let mut input = file.text.as_bytes().to_vec();
    let mut changed = false;
    for n in nodes {
        if n.is_error()
            && file.text(n) == "using"
            && n.parent().is_some_and(|p| p.kind() == "arguments")
        {
            // `using` is contextual: expect(using) is an ordinary argument.
            input[n.start_byte()] = b'_';
            changed = true;
        }
        if n.is_error() && file.text(n).bytes().all(|b| b == 0) {
            let mut parent = n.parent();
            while parent.is_some_and(|p| p.is_error()) {
                parent = parent.and_then(|p| p.parent());
            }
            if parent.is_some_and(|p| matches!(p.kind(), "template_string" | "string")) {
                input[n.byte_range()].fill(b'_');
                changed = true;
            }
        }
        if n.kind() != "call_expression"
            || n.has_error()
            || !n
                .child_by_field_name("function")
                .is_some_and(|f| f.kind() == "import")
        {
            continue;
        }
        let Some(args) = n.child_by_field_name("arguments") else {
            continue;
        };
        if args.named_child_count() != 1
            || !args.named_child(0).is_some_and(|a| a.kind() == "string")
        {
            continue;
        }
        let mut parent = n.parent();
        while parent.is_some_and(|p| matches!(p.kind(), "ERROR" | "member_expression")) {
            parent = parent.and_then(|p| p.parent());
        }
        if parent.is_some_and(|p| matches!(p.kind(), "type_arguments" | "type_annotation")) {
            // Import types have no runtime behavior. Stand in for the module type
            // with an identifier; annotations are still read from the original text.
            for byte in &mut input[n.byte_range()] {
                if !matches!(*byte, b'\n' | b'\r') {
                    *byte = b' ';
                }
            }
            input[n.start_byte()] = b'_';
            changed = true;
        }
    }
    if changed {
        String::from_utf8(input).ok()
    } else {
        None
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
    load_under(root, root)
}
/// Parse only the requested directory, retaining repository-relative source paths.
pub fn load_under(root: &Path, directory: &Path) -> Result<BTreeMap<String, SourceFile>> {
    scan_under(root, directory)?
        .into_iter()
        .map(|file| {
            let file = file?;
            Ok((file.path.clone(), file))
        })
        .collect()
}
/// Discovery may report an incomplete scan; extraction continues to use strict loading.
pub fn discover_under(
    root: &Path,
    directory: &Path,
) -> Result<(BTreeMap<String, SourceFile>, Vec<String>)> {
    let mut files = BTreeMap::new();
    let mut warnings = Vec::new();
    for file in scan_under(root, directory)? {
        match file {
            Ok(file) => {
                files.insert(file.path.clone(), file);
            }
            Err(error) => warnings.push(format!(
                "{error:#}; entrypoints from this file were skipped"
            )),
        }
    }
    Ok((files, warnings))
}
fn scan_under(root: &Path, directory: &Path) -> Result<Vec<Result<SourceFile>>> {
    let paths = paths_under(root, directory)?;
    Ok(paths
        .par_iter()
        .map(|p| {
            let text =
                std::fs::read_to_string(p).with_context(|| format!("{}:1: read", p.display()))?;
            SourceFile::parse(
                p.strip_prefix(root)?.to_string_lossy().replace('\\', "/"),
                text,
            )
        })
        .collect())
}
/// Source candidates without parsing: indexers understand their own language syntax.
pub fn paths(root: &Path) -> Result<Vec<PathBuf>> {
    paths_under(root, root)
}
/// A nested Git checkout/worktree is a separate project. An explicitly requested
/// scan root remains eligible, including when its .git marker is a worktree file.
pub(crate) fn within_project(entry: &walkdir::DirEntry) -> bool {
    entry.depth() == 0 || !entry.file_type().is_dir() || !entry.path().join(".git").exists()
}
fn paths_under(root: &Path, directory: &Path) -> Result<Vec<PathBuf>> {
    anyhow::ensure!(
        directory.is_dir(),
        "{}:1: not a directory",
        directory.display()
    );
    anyhow::ensure!(
        directory.starts_with(root),
        "{}:1: outside repository root",
        directory.display()
    );
    let mut paths = vec![];
    for entry in walkdir::WalkDir::new(directory)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            within_project(e)
                && !matches!(
                    e.file_name().to_str(),
                    Some(
                        "node_modules"
                            | ".git"
                            | ".wtflow"
                            | ".gradle"
                            | "build"
                            | "dist"
                            | "target"
                            | ".venv"
                            | "__pycache__"
                    )
                )
        })
    {
        let entry = entry.with_context(|| format!("{}:1: scan", directory.display()))?;
        if entry.file_type().is_file()
            && Language::from_path(entry.path()).is_some()
            && !entry.file_name().to_string_lossy().ends_with(".d.ts")
        {
            paths.push(entry.into_path());
        }
    }
    paths.sort();
    Ok(paths)
}
pub fn normalized(s: &str) -> String {
    // Collapse whitespace outside quoted literals, preserving literal contents.
    let mut out = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut space = false;
    for c in s.trim().trim_end_matches(';').chars() {
        if let Some(q) = quote {
            match c {
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                _ => out.push(c),
            }
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
