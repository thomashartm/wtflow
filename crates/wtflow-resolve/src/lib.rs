//! Language-independent call resolution. Heuristic facts are gathered from the AST.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub type RelPath = String;
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ByteRange {
    pub start: usize,
    pub end: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Def {
    pub symbol: String,
    pub file: RelPath,
    pub range: ByteRange,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Resolution {
    Def {
        symbol: String,
        file: RelPath,
        range: ByteRange,
    },
    Impls {
        symbol: String,
        impls: Vec<Def>,
    },
    External {
        symbol: String,
        package: String,
    },
    Unresolved,
}
impl From<Def> for Resolution {
    fn from(d: Def) -> Self {
        Self::Def {
            symbol: d.symbol,
            file: d.file,
            range: d.range,
        }
    }
}
pub trait Resolver: Send + Sync {
    fn resolve(&self, file: &RelPath, callee: ByteRange) -> Resolution;
    fn implementations(&self, symbol: &str) -> Vec<String>;
}
#[derive(Default)]
pub struct HeuristicResolver {
    calls: BTreeMap<(RelPath, ByteRange), Def>,
}
impl HeuristicResolver {
    pub fn new(calls: BTreeMap<(RelPath, ByteRange), Def>) -> Self {
        Self { calls }
    }
}
impl Resolver for HeuristicResolver {
    fn resolve(&self, file: &RelPath, callee: ByteRange) -> Resolution {
        self.calls
            .get(&(file.clone(), callee))
            .cloned()
            .map(Resolution::from)
            .unwrap_or(Resolution::Unresolved)
    }
    fn implementations(&self, _symbol: &str) -> Vec<String> {
        vec![]
    }
}
