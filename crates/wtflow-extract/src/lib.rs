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
use rayon::prelude::*;
pub use source::SourceFile;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use wtflow_core::{Entry, Flow, Kind, Node, ResolutionMode, State};
use wtflow_resolve::{chain::ChainResolver, Resolver};
/// Follow application calls by default; explicit --depth remains available for summaries.
pub const DEFAULT_DEPTH: usize = 32;
const MAX_EXPANSIONS: usize = 512;
pub struct Extraction {
    pub flow: Flow,
    pub notes: Vec<String>,
}
pub struct Scope {
    pub depth: usize,
    pub max_depth: usize,
    pub owner: String,
    pub path: Vec<String>,
    pub heuristic_used: bool,
    expansions: usize,
    notes: BTreeSet<String>,
}
impl Scope {
    fn enter(&mut self, key: String, src: &str) -> bool {
        let reason = if self.path.contains(&key) {
            Some("recursive call")
        } else if self.depth >= self.max_depth {
            Some("depth limit")
        } else if self.expansions >= MAX_EXPANSIONS {
            Some("flow size limit")
        } else {
            None
        };
        if let Some(reason) = reason {
            self.notes
                .insert(format!("{src}: {reason}; left {key} as a visible call"));
            return false;
        }
        self.expansions += 1;
        self.depth += 1;
        self.path.push(key);
        true
    }
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
                entrypoints::detect(
                    cx.funcs
                        .get(&file.path)
                        .map(Vec::as_slice)
                        .unwrap_or_default(),
                    file,
                )
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
    pub chain: ChainResolver,
    pub metadata: Option<wtflow_resolve::metadata::Metadata>,
}
/// Parsed discovery inventory. Call resolution is prepared only for extraction.
pub struct Discovery {
    pub config: config::RepositoryConfig,
    files: BTreeMap<String, SourceFile>,
    funcs: BTreeMap<String, Vec<Func>>,
}
impl Discovery {
    pub fn entrypoints(&self) -> Vec<EntryPoint> {
        detect_entries(&self.files, &self.funcs)
    }
    pub fn into_context(self) -> Result<Cx> {
        Cx::with_functions(self.config, self.files, self.funcs)
    }
}
fn collect_functions(files: &BTreeMap<String, SourceFile>) -> BTreeMap<String, Vec<Func>> {
    files
        .par_iter()
        .map(|(path, file)| (path.clone(), functions::collect(file)))
        .collect()
}
fn detect_entries(
    files: &BTreeMap<String, SourceFile>,
    funcs: &BTreeMap<String, Vec<Func>>,
) -> Vec<EntryPoint> {
    let mut entries: Vec<_> = files
        .par_iter()
        .flat_map_iter(|(path, file)| {
            entrypoints::detect(funcs.get(path).map(Vec::as_slice).unwrap_or_default(), file)
        })
        .collect();
    entries.sort_by(|a, b| (&a.file, &a.symbol, &a.trigger).cmp(&(&b.file, &b.symbol, &b.trigger)));
    entries
}
impl Cx {
    pub fn load(entry: &Path) -> Result<Self> {
        let config = config::RepositoryConfig::discover(entry)?;
        let files = source::load(&config.root)?;
        Self::from_files(config, files)
    }
    /// Discover configuration upwards, but scan entrypoints only under `directory`.
    pub fn load_entrypoints(directory: &Path) -> Result<(Discovery, Vec<String>)> {
        let directory = directory
            .canonicalize()
            .with_context(|| format!("{}:1: entrypoint directory", directory.display()))?;
        let config = config::RepositoryConfig::discover(&directory)?;
        let (files, warnings) = source::discover_under(&config.root, &directory)?;
        let funcs = collect_functions(&files);
        Ok((
            Discovery {
                config,
                files,
                funcs,
            },
            warnings,
        ))
    }
    fn from_files(
        config: config::RepositoryConfig,
        files: BTreeMap<String, SourceFile>,
    ) -> Result<Self> {
        let funcs = collect_functions(&files);
        Self::with_functions(config, files, funcs)
    }
    fn with_functions(
        config: config::RepositoryConfig,
        files: BTreeMap<String, SourceFile>,
        funcs: BTreeMap<String, Vec<Func>>,
    ) -> Result<Self> {
        let heuristic = heuristic::build(&files, &funcs, &config.root)?;
        let metadata = wtflow_resolve::metadata::Metadata::load(&config.root)?;
        Ok(Self {
            config,
            files,
            funcs,
            chain: ChainResolver {
                heuristic,
                scip: None,
                stale: BTreeSet::new(),
            },
            metadata,
        })
    }
    pub fn resolver(&self) -> &dyn Resolver {
        &self.chain
    }
    pub fn enable_scip(&mut self, required: bool) -> Result<()> {
        let mut indexes = vec![];
        for entry in walkdir::WalkDir::new(&self.config.root)
            .into_iter()
            .filter_entry(|e| {
                source::within_project(e)
                    && !matches!(
                        e.file_name().to_str(),
                        Some("node_modules" | "target" | "build" | ".git" | ".gradle" | ".venv")
                    )
            })
        {
            let entry = entry?;
            if entry.file_type().is_file() && entry.path().extension().is_some_and(|e| e == "scip")
            {
                indexes.push(entry.into_path());
            }
        }
        anyhow::ensure!(
            !required || !indexes.is_empty(),
            "no SCIP index; run wtflow index first"
        );
        if indexes.is_empty() {
            return Ok(());
        }
        indexes.sort();
        let mut metadata = self.metadata.take().unwrap_or_default();
        for index in &indexes {
            if let Some(base) = index
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
            {
                if let Some(meta) = wtflow_resolve::metadata::Metadata::load(base)? {
                    let prefix = base.strip_prefix(&self.config.root)?;
                    for (path, hash) in meta.files {
                        metadata
                            .files
                            .insert(prefix.join(path).to_string_lossy().replace('\\', "/"), hash);
                    }
                }
            }
        }
        self.metadata = Some(metadata);
        self.chain.stale = self
            .files
            .iter()
            .filter(|(p, f)| {
                self.metadata
                    .as_ref()
                    .map_or(true, |m| !m.fresh(p, f.text.as_bytes()))
            })
            .map(|(p, _)| p.clone())
            .collect();
        let sources = self
            .files
            .iter()
            .map(|(p, f)| (p.clone(), std::sync::Arc::<str>::from(f.text.as_str())))
            .collect();
        self.chain.scip = Some(wtflow_resolve::scip::ScipResolver::load_with_metadata(
            &self.config.root,
            &indexes,
            &sources,
            self.metadata.as_ref(),
        )?);
        Ok(())
    }
    pub fn stale_for(&self, flow: &Flow) -> Vec<String> {
        let Some(meta) = &self.metadata else {
            return vec![];
        };
        let mut paths = BTreeSet::from([flow.entry.file.clone()]);
        let mut nodes = vec![];
        wtflow_core::visit(&flow.steps, &mut nodes);
        for n in nodes {
            if let Some((path, _)) = n.src.rsplit_once(':') {
                paths.insert(path.into());
            }
        }
        paths
            .into_iter()
            .filter(|p| {
                self.files
                    .get(p)
                    .map_or(true, |f| !meta.fresh(p, f.text.as_bytes()))
            })
            .collect()
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
        detect_entries(&self.files, &self.funcs)
    }
    pub fn extract(
        &self,
        file: &str,
        symbol: &str,
        name: Option<&str>,
        depth: usize,
    ) -> Result<Flow> {
        Ok(self.extract_report(file, symbol, name, depth)?.flow)
    }
    pub fn extract_report(
        &self,
        file: &str,
        symbol: &str,
        name: Option<&str>,
        depth: usize,
    ) -> Result<Extraction> {
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
            heuristic_used: false,
            expansions: 0,
            notes: BTreeSet::new(),
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
            resolution: if self.chain.scip.is_none() || self.chain.stale.contains(file) {
                ResolutionMode::Heuristic
            } else if scope.heuristic_used {
                ResolutionMode::Mixed
            } else {
                ResolutionMode::Scip
            },
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
        Ok(Extraction {
            flow,
            notes: scope.notes.into_iter().collect(),
        })
    }
}
pub mod context;
