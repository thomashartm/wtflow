//! Deterministic flow documents. Structure is owned by extraction; labels by humans.
pub mod fingerprint;
pub mod ids;
pub mod schema;
pub mod yaml;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Do,
    Call,
    Emit,
    Wait,
    Group,
    If,
    Switch,
    ForEach,
    While,
    Parallel,
    Try,
    Return,
    Fail,
    Break,
    Continue,
}
impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Do => "do",
            Self::Call => "call",
            Self::Emit => "emit",
            Self::Wait => "wait",
            Self::Group => "group",
            Self::If => "if",
            Self::Switch => "switch",
            Self::ForEach => "for_each",
            Self::While => "while",
            Self::Parallel => "parallel",
            Self::Try => "try",
            Self::Return => "return",
            Self::Fail => "fail",
            Self::Break => "break",
            Self::Continue => "continue",
        }
    }
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Return | Self::Fail | Self::Break | Self::Continue
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: String,
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub code: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub src: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reads: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub writes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub then: Vec<Node>,
    #[serde(default, rename = "else", skip_serializing_if = "Vec::is_empty")]
    pub otherwise: Vec<Node>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cases: Vec<Case>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default: Vec<Node>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub body: Vec<Node>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub catch: Vec<Node>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub finally: Vec<Node>,
}
impl Node {
    pub fn new(kind: Kind, code: impl Into<String>) -> Self {
        Self {
            id: String::new(),
            kind,
            code: code.into(),
            label: None,
            src: String::new(),
            target: None,
            symbol: None,
            boundary: None,
            topic: None,
            tx: None,
            reads: vec![],
            writes: vec![],
            then: vec![],
            otherwise: vec![],
            cases: vec![],
            default: vec![],
            body: vec![],
            catch: vec![],
            finally: vec![],
        }
    }
    pub fn children(&self) -> impl Iterator<Item = &Node> {
        self.then
            .iter()
            .chain(&self.otherwise)
            .chain(self.cases.iter().flat_map(|c| &c.steps))
            .chain(&self.default)
            .chain(&self.body)
            .chain(&self.catch)
            .chain(&self.finally)
    }
    pub fn children_mut(&mut self) -> impl Iterator<Item = &mut Node> {
        self.then
            .iter_mut()
            .chain(&mut self.otherwise)
            .chain(self.cases.iter_mut().flat_map(|c| &mut c.steps))
            .chain(&mut self.default)
            .chain(&mut self.body)
            .chain(&mut self.catch)
            .chain(&mut self.finally)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub when: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallthrough: Option<bool>,
    pub steps: Vec<Node>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub lang: String,
    pub file: String,
    pub symbol: String,
    pub depth: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub reads: Vec<String>,
    pub writes: Vec<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionMode {
    Scip,
    Heuristic,
    Mixed,
}
impl ResolutionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Scip => "scip",
            Self::Heuristic => "heuristic",
            Self::Mixed => "mixed",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Flow {
    pub flow: String,
    pub version: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub owner: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub trigger: String,
    pub entry: Entry,
    pub resolution: ResolutionMode,
    pub fingerprint: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<Input>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub output: String,
    #[serde(default)]
    pub state: State,
    #[serde(default)]
    pub boundaries: Vec<String>,
    pub steps: Vec<Node>,
}
impl Flow {
    pub fn refresh_fingerprint(&mut self) -> anyhow::Result<()> {
        self.fingerprint = fingerprint::compute(&self.steps)?;
        Ok(())
    }
    pub fn verify_fingerprint(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.fingerprint == fingerprint::compute(&self.steps)?,
            "E003 fingerprint mismatch"
        );
        Ok(())
    }
}
pub fn visit<'a>(nodes: &'a [Node], out: &mut Vec<&'a Node>) {
    for n in nodes {
        out.push(n);
        for c in n.children() {
            visit(std::slice::from_ref(c), out);
        }
    }
}
pub mod labels;
pub mod lint;
