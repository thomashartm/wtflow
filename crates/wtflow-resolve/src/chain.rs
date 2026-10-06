use crate::{scip::ScipResolver, ByteRange, HeuristicResolver, RelPath, Resolution, Resolver};
use std::collections::BTreeSet;
pub struct ChainResolver {
    pub scip: Option<ScipResolver>,
    pub heuristic: HeuristicResolver,
    pub stale: BTreeSet<String>,
}
impl ChainResolver {
    pub fn resolve_with_origin(&self, file: &RelPath, range: ByteRange) -> (Resolution, bool) {
        if !self.stale.contains(file) {
            if let Some(scip) = &self.scip {
                let answer = scip.resolve(file, range);
                let stale_target = match &answer {
                    Resolution::Def { file, .. } => self.stale.contains(file),
                    Resolution::Impls { impls, .. } => {
                        impls.iter().any(|d| self.stale.contains(&d.file))
                    }
                    _ => false,
                };
                if !matches!(answer, Resolution::Unresolved) && !stale_target {
                    return (answer, true);
                }
            }
        }
        (self.heuristic.resolve(file, range), false)
    }
}
impl Resolver for ChainResolver {
    fn resolve(&self, file: &RelPath, range: ByteRange) -> Resolution {
        self.resolve_with_origin(file, range).0
    }
    fn implementations(&self, symbol: &str) -> Vec<String> {
        self.scip
            .as_ref()
            .map(|s| s.implementations(symbol))
            .unwrap_or_default()
    }
}
