//! Immutable parsed source index with language adapters and a shared control-flow walker.
mod camel;
pub mod config;
pub mod entrypoints;
pub mod functions;
mod heuristic;
pub mod source;
mod walker;
use anyhow::{Context, Result};
pub use entrypoints::EntryPoint;
pub use functions::Func;
pub use source::SourceFile;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use wtflow_core::{Entry, Flow, Kind, Node, ResolutionMode, State};
use wtflow_resolve::{HeuristicResolver, Resolver};
pub struct Scope {
    pub depth: usize,
    pub max_depth: usize,
    pub owner: String,
    pub path: Vec<String>,
}
pub trait Lang: Sync {
    fn name(&self) -> &'static str;
    fn exts(&self) -> &'static [&'static str];
    fn find_symbol(&self, cx: &Cx, file: &SourceFile, symbol: &str) -> Result<Func>;
    fn body(&self, cx: &Cx, scope: &mut Scope, func: &Func) -> Vec<Node>;
    fn entrypoints(&self, cx: &Cx, file: &SourceFile) -> Vec<EntryPoint>;
}
macro_rules! adapter {
    ($name:ident,$lang:literal,$exts:expr) => {
        pub struct $name;
        impl Lang for $name {
            fn name(&self) -> &'static str {
                $lang
            }
            fn exts(&self) -> &'static [&'static str] {
                $exts
            }
            fn find_symbol(&self, cx: &Cx, file: &SourceFile, symbol: &str) -> Result<Func> {
                cx.find_symbol(&file.path, symbol)
            }
            fn body(&self, cx: &Cx, scope: &mut Scope, func: &Func) -> Vec<Node> {
                cx.body(func, scope)
            }
            fn entrypoints(&self, cx: &Cx, file: &SourceFile) -> Vec<EntryPoint> {
                entrypoints::detect(cx, file)
            }
        }
    };
}
adapter!(TypeScript, "typescript", &["ts", "tsx"]);
adapter!(Python, "python", &["py"]);
adapter!(Java, "java", &["java"]);
pub struct Cx {
    pub config: config::RepositoryConfig,
    pub files: BTreeMap<String, SourceFile>,
    pub funcs: BTreeMap<String, Vec<Func>>,
    heuristic: HeuristicResolver,
}
impl Cx {
    pub fn load(entry: &Path) -> Result<Self> {
        let config = config::RepositoryConfig::discover(entry)?;
        let files = source::load(&config.root)?;
        let funcs = files
            .iter()
            .map(|(path, f)| (path.clone(), functions::collect(f)))
            .collect();
        let heuristic = heuristic::build(&files, &funcs, &config.root)?;
        Ok(Self {
            config,
            files,
            funcs,
            heuristic,
        })
    }
    pub fn resolver(&self) -> &dyn Resolver {
        &self.heuristic
    }
    pub fn find_symbol(&self, file: &str, symbol: &str) -> Result<Func> {
        let lookup = symbol
            .split_once('@')
            .map(|(class, _)| format!("{class}.configure"))
            .unwrap_or_else(|| symbol.into());
        let matches: Vec<_> = self
            .funcs
            .get(file)
            .into_iter()
            .flatten()
            .filter(|f| f.symbol() == lookup || (f.class.is_empty() && f.name == lookup))
            .collect();
        anyhow::ensure!(
            matches.len() == 1,
            "{file}:1: symbol {symbol} has {} definitions",
            matches.len()
        );
        Ok(matches[0].clone())
    }
    pub fn body(&self, func: &Func, scope: &mut Scope) -> Vec<Node> {
        let Some(file) = self.files.get(&func.file) else {
            return vec![];
        };
        let Some(ast) = functions::node(file, func) else {
            return vec![];
        };
        if func.name == "configure" && file.text.contains("RouteBuilder") {
            return camel::extract(self, file, &func.class, None, scope);
        }
        let Some(body) = ast.child_by_field_name("body") else {
            return vec![];
        };
        let mut nodes = walker::Walker {
            cx: self,
            file,
            scope,
        }
        .walk(body);
        if self
            .config
            .config
            .tx_decorators
            .iter()
            .any(|d| func.annotations.contains(&format!("@{d}")))
        {
            let mut tx = Node::new(
                Kind::Group,
                format!(
                    "@{}",
                    self.config
                        .config
                        .tx_decorators
                        .iter()
                        .find(|d| func.annotations.contains(&format!("@{d}")))
                        .map(String::as_str)
                        .unwrap_or("Transactional")
                ),
            );
            tx.src = file.src(ast);
            tx.tx = Some("db".into());
            tx.body = nodes;
            nodes = vec![tx];
        }
        nodes
    }
    pub fn entrypoints(&self) -> Vec<EntryPoint> {
        let mut result = vec![];
        for f in self.files.values() {
            result.extend(entrypoints::detect(self, f));
        }
        result.sort_by(|a, b| {
            (&a.file, &a.symbol, &a.trigger).cmp(&(&b.file, &b.symbol, &b.trigger))
        });
        result
    }
    pub fn extract(
        &self,
        file: &str,
        symbol: &str,
        name: Option<&str>,
        depth: usize,
    ) -> Result<Flow> {
        let func = self.find_symbol(file, symbol)?;
        let source = self
            .files
            .get(file)
            .with_context(|| format!("{file}:1: source not found"))?;
        let owner = self.config.owner(file);
        let mut scope = Scope {
            depth: 0,
            max_depth: depth,
            owner: owner.clone(),
            path: vec![format!("{file}#{}", func.symbol())],
        };
        let (mut steps, trigger, inputs, output) = if let Some((class, id)) = symbol.split_once('@')
        {
            let ep = self
                .entrypoints()
                .into_iter()
                .find(|e| e.file == file && e.symbol == symbol)
                .with_context(|| format!("{file}:1: route {symbol} not found"))?;
            (
                camel::extract(self, source, class, Some(id), &mut scope),
                ep.trigger,
                vec![],
                String::new(),
            )
        } else {
            (
                self.body(&func, &mut scope),
                entrypoints::trigger(source, &func),
                func.inputs.clone(),
                func.output.clone(),
            )
        };
        wtflow_core::ids::assign(&mut steps);
        let mut nodes = vec![];
        wtflow_core::visit(&steps, &mut nodes);
        let reads: BTreeSet<_> = nodes.iter().flat_map(|n| n.reads.clone()).collect();
        let writes: BTreeSet<_> = nodes.iter().flat_map(|n| n.writes.clone()).collect();
        let boundaries: BTreeSet<_> = nodes.iter().filter_map(|n| n.boundary.clone()).collect();
        let mut flow = Flow {
            flow: name.unwrap_or(symbol).into(),
            version: 1,
            owner,
            trigger,
            entry: Entry {
                lang: source.lang.name().into(),
                file: file.into(),
                symbol: symbol.into(),
                depth,
            },
            resolution: ResolutionMode::Heuristic,
            fingerprint: String::new(),
            inputs,
            output,
            state: State {
                reads: reads.into_iter().collect(),
                writes: writes.into_iter().collect(),
            },
            boundaries: boundaries.into_iter().collect(),
            steps,
        };
        flow.refresh_fingerprint()?;
        Ok(flow)
    }
}
