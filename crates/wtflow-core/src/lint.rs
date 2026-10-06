use crate::{Flow, Kind, Node};
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}
impl Severity {
    fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub rule: String,
    pub node_id: String,
    pub message: String,
    #[serde(skip)]
    pub order: usize,
}
impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {} {} {}",
            self.severity.as_str(),
            self.rule,
            if self.node_id.is_empty() {
                "-"
            } else {
                &self.node_id
            },
            self.message
        )
    }
}
#[derive(Default)]
pub struct Context {
    pub verbose: bool,
    pub source_changed: bool,
    pub source: bool,
    pub stale_files: Vec<String>,
}
fn diag(
    severity: Severity,
    rule: &str,
    id: &str,
    message: impl Into<String>,
    order: usize,
) -> Diagnostic {
    Diagnostic {
        severity,
        rule: rule.into(),
        node_id: id.into(),
        message: message.into(),
        order,
    }
}
pub fn fails(ds: &[Diagnostic], strict: bool) -> bool {
    ds.iter()
        .any(|d| d.severity == Severity::Error || (strict && d.severity == Severity::Warning))
}
fn finishes(nodes: &[Node]) -> bool {
    nodes.last().is_some_and(|n| match n.kind {
        Kind::Return | Kind::Fail | Kind::Break | Kind::Continue => true,
        Kind::If => !n.otherwise.is_empty() && finishes(&n.then) && finishes(&n.otherwise),
        Kind::Switch => {
            !n.default.is_empty()
                && finishes(&n.default)
                && n.cases.iter().all(|c| finishes(&c.steps))
        }
        Kind::Try => {
            finishes(&n.finally)
                || (finishes(&n.body) && (n.catch.is_empty() || finishes(&n.catch)))
        }
        _ => false,
    })
}
fn has_loop_exit(nodes: &[Node]) -> bool {
    nodes.iter().any(|n| match n.kind {
        Kind::Break | Kind::Return | Kind::Fail => true,
        Kind::While | Kind::ForEach | Kind::Group => false,
        _ => n.children().any(|c| has_loop_exit(std::slice::from_ref(c))),
    })
}
struct Checker<'a> {
    flow: &'a Flow,
    ctx: &'a Context,
    diagnostics: Vec<Diagnostic>,
    ids: BTreeSet<String>,
    order: usize,
    writes: Vec<(usize, String)>,
    missing: usize,
    unresolved: usize,
}
impl Checker<'_> {
    fn add(&mut self, rule: &str, n: &Node, message: impl Into<String>) {
        self.diagnostics
            .push(diag(Severity::Warning, rule, &n.id, message, self.order));
    }
    fn nodes(&mut self, nodes: &[Node], tx: bool) {
        let mut dead = false;
        for n in nodes {
            self.order += 1;
            if dead {
                self.add("W101", n, "unreachable after a terminal");
            }
            if n.id.is_empty() || !self.ids.insert(n.id.clone()) {
                self.diagnostics.push(diag(
                    Severity::Error,
                    "E002",
                    &n.id,
                    "missing or duplicate id",
                    self.order,
                ));
            }
            if n.label.as_ref().map_or(true, |s| s.is_empty()) {
                self.missing += 1;
            }
            if n.kind == Kind::Do && n.code != "(error ignored)" {
                self.unresolved += 1;
                if self.ctx.verbose {
                    self.diagnostics.push(diag(
                        Severity::Info,
                        "I002",
                        &n.id,
                        "unresolved call",
                        self.order,
                    ));
                }
            }
            let tx = tx || n.tx.is_some();
            if !tx && !n.writes.is_empty() {
                self.writes.push((self.order, n.id.clone()));
            }
            if tx && n.kind == Kind::Emit {
                self.add("W110", n, "emit inside a transaction");
            }
            if tx
                && n.boundary.as_ref().is_some_and(|b| b != &self.flow.owner)
                && matches!(n.kind, Kind::Call | Kind::Group)
            {
                self.add("W112", n, "foreign-boundary call inside a transaction");
            }
            match n.kind {
                Kind::Switch => {
                    if n.code.starts_with("dispatch ") && n.cases.len() > 1 {
                        self.add("W113", n, "ambiguous dispatch: several implementations");
                    } else if n.default.is_empty() {
                        self.add("W102", n, "switch without default");
                    }
                    if n.cases.iter().any(|c| c.fallthrough == Some(true)) {
                        self.add("W103", n, "switch case falls through");
                    }
                }
                Kind::Try if n.catch.iter().any(|c| c.code == "(error ignored)") => {
                    self.add("W104", n, "swallowed error")
                }
                Kind::While => {
                    if matches!(
                        n.code.trim(),
                        "true" | "True" | "1" | "for(;;)" | "for (;;)"
                    ) && !has_loop_exit(&n.body)
                    {
                        self.add("W106", n, "unconditional loop without exit");
                    }
                    if n.body.is_empty() {
                        self.add("W107", n, "empty loop");
                    }
                }
                Kind::ForEach if n.body.is_empty() => self.add("W107", n, "empty loop"),
                Kind::If if n.then.is_empty() && n.otherwise.is_empty() => {
                    self.add("W107", n, "empty if")
                }
                _ => {}
            }
            for children in [&n.then, &n.otherwise] {
                self.nodes(children, tx);
            }
            for c in &n.cases {
                self.nodes(&c.steps, tx);
            }
            for children in [&n.default, &n.body, &n.catch, &n.finally] {
                self.nodes(children, tx);
            }
            dead = dead || finishes(std::slice::from_ref(n));
        }
    }
}
pub fn check(flow: &Flow, ctx: &Context) -> Vec<Diagnostic> {
    let mut c = Checker {
        flow,
        ctx,
        diagnostics: vec![],
        ids: BTreeSet::new(),
        order: 0,
        writes: vec![],
        missing: 0,
        unresolved: 0,
    };
    if flow.version != 1 {
        c.diagnostics
            .push(diag(Severity::Error, "E000", "", "unsupported version", 0));
    }
    if flow.verify_fingerprint().is_err() {
        c.diagnostics
            .push(diag(Severity::Error, "E003", "", "fingerprint mismatch", 0));
    }
    if ctx.source_changed {
        c.diagnostics
            .push(diag(Severity::Error, "E005", "", "stale vs. source", 0));
    }
    let mut stale = ctx.stale_files.clone();
    stale.sort();
    stale.dedup();
    for file in stale {
        c.diagnostics.push(diag(
            if ctx.source {
                Severity::Error
            } else {
                Severity::Warning
            },
            "W120",
            "",
            format!("stale index for {file}"),
            0,
        ));
    }
    c.nodes(&flow.steps, false);
    if c.writes.len() > 1 {
        let (order, id) = &c.writes[0];
        c.diagnostics.push(diag(
            Severity::Warning,
            "W105",
            id,
            "more than one write outside a transaction",
            *order,
        ));
    }
    if c.missing > 0 {
        c.diagnostics.push(diag(
            Severity::Info,
            "I001",
            "",
            format!("{} missing labels", c.missing),
            usize::MAX,
        ));
    }
    if c.unresolved > 0 {
        c.diagnostics.push(diag(
            Severity::Info,
            "I002",
            "",
            format!("{} unresolved calls", c.unresolved),
            usize::MAX,
        ));
    }
    c.diagnostics.sort_by_key(|d| (d.severity, d.order));
    c.diagnostics
}
/// Inspect malformed documents without losing the specific structural diagnostic to serde.
pub fn document(text: &str, file: &str, ctx: &Context) -> Vec<Diagnostic> {
    let value: serde_json::Value = match serde_yaml_ng::from_str(text) {
        Ok(v) => v,
        Err(e) => return vec![diag(Severity::Error, "E000", "", format!("{file}: {e}"), 0)],
    };
    if value.get("version").and_then(|v| v.as_u64()) != Some(1) {
        return vec![diag(
            Severity::Error,
            "E000",
            "",
            format!("{file}:1: missing or unsupported version"),
            0,
        )];
    }
    let mut diagnostics = Vec::new();
    let mut order = 0;
    let mut ids = BTreeSet::new();
    fn scan(
        v: &serde_json::Value,
        ds: &mut Vec<Diagnostic>,
        order: &mut usize,
        ids: &mut BTreeSet<String>,
    ) {
        if let Some(nodes) = v.as_array() {
            for n in nodes {
                *order += 1;
                let id = n.get("id").and_then(|v| v.as_str()).unwrap_or("");
                if id.is_empty() || !ids.insert(id.into()) {
                    ds.push(diag(
                        Severity::Error,
                        "E002",
                        id,
                        "missing or duplicate id",
                        *order,
                    ));
                }
                let kind = n.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                let allowed: &[&str] = match kind {
                    "if" => &["then", "else"],
                    "switch" => &["cases", "default"],
                    "while" | "for_each" | "group" | "parallel" => &["body"],
                    "try" => &["body", "catch", "finally"],
                    "do" | "call" | "emit" | "wait" | "return" | "fail" | "break" | "continue" => {
                        &[]
                    }
                    _ => {
                        ds.push(diag(Severity::Error, "E001", id, "unknown kind", *order));
                        &[]
                    }
                };
                for k in [
                    "then", "else", "cases", "default", "body", "catch", "finally",
                ] {
                    if let Some(v) = n.get(k) {
                        if !allowed.contains(&k) {
                            ds.push(diag(
                                Severity::Error,
                                "E004",
                                id,
                                format!("child list {k} not allowed for {kind}"),
                                *order,
                            ));
                        }
                        if k == "cases" {
                            if let Some(cases) = v.as_array() {
                                for case in cases {
                                    scan(&case["steps"], ds, order, ids);
                                }
                            }
                        } else {
                            scan(v, ds, order, ids);
                        }
                    }
                }
            }
        }
    }
    scan(&value["steps"], &mut diagnostics, &mut order, &mut ids);
    if let Err(e) = crate::schema::validate(&value, false) {
        diagnostics.push(diag(
            Severity::Error,
            "E006",
            "",
            format!("{file}:1: {e}"),
            0,
        ));
    }
    if diagnostics.is_empty() {
        match serde_json::from_value::<Flow>(value) {
            Ok(f) => return check(&f, ctx),
            Err(e) => diagnostics.push(diag(
                Severity::Error,
                "E000",
                "",
                format!("{file}:1: {e}"),
                0,
            )),
        }
    }
    diagnostics.sort_by_key(|d| (d.severity, d.order));
    diagnostics
}
