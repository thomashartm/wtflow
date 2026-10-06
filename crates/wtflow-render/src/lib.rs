//! Mermaid rendering by exit-frontier propagation, independent of source languages.
use wtflow_core::{Flow, Kind, Node};
#[derive(Clone, Copy, Default)]
pub enum Language {
    #[default]
    En,
    De,
}
#[derive(Default)]
pub struct Options {
    pub lang: Language,
    pub detail: bool,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Exit {
    Normal,
    Return,
    Fail,
    Break,
    Continue,
}
#[derive(Clone)]
struct Frontier {
    id: String,
    exit: Exit,
    label: Option<String>,
}
impl Frontier {
    fn normal(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            exit: Exit::Normal,
            label: None,
        }
    }
}
struct Renderer<'a> {
    options: &'a Options,
    output: String,
    next: usize,
    indent: usize,
    edges: std::collections::BTreeSet<String>,
}
fn escape(s: &str) -> String {
    s.replace('&', "#amp;")
        .replace('"', "#quot;")
        .replace('<', "#lt;")
        .replace('>', "#gt;")
        .replace('\n', " ")
}
fn truncate(s: &str) -> String {
    if s.chars().count() > 60 {
        format!("{}…", s.chars().take(59).collect::<String>())
    } else {
        s.into()
    }
}
impl Renderer<'_> {
    fn word(&self, en: &'static str, de: &'static str) -> &'static str {
        match self.options.lang {
            Language::En => en,
            Language::De => de,
        }
    }
    fn line(&mut self, s: impl AsRef<str>) {
        self.output.push_str(&"  ".repeat(self.indent));
        self.output.push_str(s.as_ref());
        self.output.push('\n');
    }
    fn edge(&mut self, from: &Frontier, to: &str) {
        let edge = if let Some(label) = &from.label {
            format!("{} -->|\"{}\"| {to}", from.id, escape(label))
        } else {
            format!("{} --> {to}", from.id)
        };
        if self.edges.insert(edge.clone()) {
            self.line(edge);
        }
    }
    fn text(&self, n: &Node) -> String {
        let text = n.label.as_deref().unwrap_or(&n.code);
        let mut text = escape(&truncate(text));
        if self.options.detail && n.label.is_some() {
            text.push_str("<br/>");
            text.push_str(&escape(&truncate(&n.code)));
        }
        if !n.reads.is_empty() {
            text.push_str("<br/>⛁ ");
            text.push_str(&escape(&n.reads.join(", ")));
        }
        if !n.writes.is_empty() {
            text.push_str("<br/>✎ ");
            text.push_str(&escape(&n.writes.join(", ")));
        }
        text
    }
    fn sequence(&mut self, nodes: &[Node], incoming: Vec<Frontier>) -> Vec<Frontier> {
        let mut frontier = incoming;
        for n in nodes {
            let (normal, mut stopped): (Vec<_>, Vec<_>) =
                frontier.into_iter().partition(|f| f.exit == Exit::Normal);
            stopped.extend(self.node(n, normal));
            frontier = stopped;
        }
        frontier
    }
    fn node(&mut self, n: &Node, incoming: Vec<Frontier>) -> Vec<Frontier> {
        self.next += 1;
        let id = format!("n{}", self.next);
        let text = self.text(n);
        for from in &incoming {
            self.edge(from, &id);
        }
        match n.kind {
            Kind::Group | Kind::Try | Kind::Parallel => {
                self.line(format!("subgraph {id}[\"{text}\"]"));
                self.indent += 1;
                let mut exits = if n.kind == Kind::Parallel {
                    let mut branches = vec![];
                    for child in &n.body {
                        branches.extend(self.node(child, vec![]));
                    }
                    branches
                } else {
                    self.sequence(&n.body, vec![])
                };
                // Empty groups have a usable graph-level exit.
                if n.body.is_empty() {
                    exits.push(Frontier::normal(&id));
                }
                self.indent -= 1;
                self.line("end");
                if n.tx.is_some() {
                    self.line(format!(
                        "style {id} fill:#fff4d6,stroke:#b88700,stroke-width:2px"
                    ));
                }
                if n.kind == Kind::Try && !n.catch.is_empty() {
                    let catch_id = format!("n{}", self.next + 1);
                    let handler = self.sequence(&n.catch, vec![]);
                    self.line(format!(
                        "{id} -.->|\"{}\"| {catch_id}",
                        self.word("Error", "Fehler")
                    ));
                    exits.retain(|f| f.exit != Exit::Fail);
                    exits.extend(handler);
                }
                if n.kind == Kind::Try && !n.finally.is_empty() {
                    let pending: Vec<_> = exits.iter().map(|f| f.exit).collect();
                    let normalized = exits
                        .into_iter()
                        .map(|mut f| {
                            f.exit = Exit::Normal;
                            f
                        })
                        .collect();
                    let cleanup = self.sequence(&n.finally, normalized);
                    exits = vec![];
                    for f in cleanup {
                        if f.exit != Exit::Normal {
                            exits.push(f);
                        } else {
                            for kind in &pending {
                                let mut resume = f.clone();
                                resume.exit = *kind;
                                exits.push(resume);
                            }
                        }
                    }
                }
                if n.kind == Kind::Group {
                    for f in &mut exits {
                        if f.exit == Exit::Return {
                            f.exit = Exit::Normal;
                        }
                    }
                }
                exits
            }
            Kind::If => {
                self.line(format!("{id}{{\"{text}\"}}"));
                let yes = Frontier {
                    id: id.clone(),
                    exit: Exit::Normal,
                    label: Some(self.word("yes", "ja").into()),
                };
                let no = Frontier {
                    id,
                    exit: Exit::Normal,
                    label: Some(self.word("no", "nein").into()),
                };
                let mut exits = self.sequence(&n.then, vec![yes]);
                exits.extend(self.sequence(&n.otherwise, vec![no]));
                exits
            }
            Kind::Switch => {
                self.line(format!("{id}{{\"{text}\"}}"));
                let mut exits = vec![];
                let mut fall = vec![];
                for case in &n.cases {
                    let mut incoming = vec![Frontier {
                        id: id.clone(),
                        exit: Exit::Normal,
                        label: Some(case.when.clone()),
                    }];
                    incoming.append(&mut fall);
                    let branch = self.sequence(&case.steps, incoming);
                    for mut f in branch {
                        if f.exit == Exit::Break {
                            f.exit = Exit::Normal;
                            exits.push(f);
                        } else if f.exit == Exit::Normal && case.fallthrough == Some(true) {
                            fall.push(f);
                        } else {
                            exits.push(f);
                        }
                    }
                }
                if n.default.is_empty() && n.code.starts_with("dispatch ") {
                    exits.extend(fall);
                } else {
                    fall.push(Frontier {
                        id,
                        exit: Exit::Normal,
                        label: Some(self.word("default", "sonst").into()),
                    });
                    for mut f in self.sequence(&n.default, fall) {
                        if f.exit == Exit::Break {
                            f.exit = Exit::Normal;
                        }
                        exits.push(f);
                    }
                }
                exits
            }
            Kind::ForEach | Kind::While => {
                self.line(format!("{id}{{{{\"{text}\"}}}}"));
                let head = Frontier {
                    id: id.clone(),
                    exit: Exit::Normal,
                    label: Some(self.word("next", "nächstes").into()),
                };
                let body = self.sequence(&n.body, vec![head]);
                let mut exits = vec![Frontier {
                    id: id.clone(),
                    exit: Exit::Normal,
                    label: Some(self.word("done", "fertig").into()),
                }];
                for mut f in body {
                    match f.exit {
                        Exit::Normal | Exit::Continue => self.edge(&f, &id),
                        Exit::Break => {
                            f.exit = Exit::Normal;
                            exits.push(f);
                        }
                        _ => exits.push(f),
                    }
                }
                exits
            }
            _ => {
                let shape = match n.kind {
                    Kind::Call => format!(
                        "[[\"{}{}\"]]:::k_call",
                        n.boundary
                            .as_ref()
                            .map(|b| format!("{}: ", escape(b)))
                            .unwrap_or_default(),
                        text
                    ),
                    Kind::Emit => format!(
                        ">\"{}{}\"]:::k_emit",
                        text,
                        n.topic
                            .as_ref()
                            .map(|t| format!(" → {}", escape(t)))
                            .unwrap_or_default()
                    ),
                    Kind::Wait => format!("[/\"{text}\"/]"),
                    Kind::Return => format!("([\"{text}\"]):::k_term"),
                    Kind::Fail => format!("([\"{text}\"]):::k_fail"),
                    _ => format!("[\"{text}\"]"),
                };
                self.line(format!("{id}{shape}"));
                let exit = match n.kind {
                    Kind::Return => Exit::Return,
                    Kind::Fail => Exit::Fail,
                    Kind::Break => Exit::Break,
                    Kind::Continue => Exit::Continue,
                    _ => Exit::Normal,
                };
                vec![Frontier {
                    id,
                    exit,
                    label: None,
                }]
            }
        }
    }
}
pub fn render(flow: &Flow, options: &Options) -> anyhow::Result<String> {
    let mut r = Renderer {
        options,
        output: String::new(),
        next: 0,
        indent: 0,
        edges: std::collections::BTreeSet::new(),
    };
    r.line("---");
    r.line(format!("title: {}", wtflow_core::yaml::quote(&flow.flow)?));
    r.line("---");
    r.line("flowchart TD");
    r.indent = 1;
    r.line(format!("start([\"{}\"])", r.word("Start", "Start")));
    r.line(format!("finish([\"{}\"])", r.word("Finish", "Ende")));
    let exits = r.sequence(&flow.steps, vec![Frontier::normal("start")]);
    for f in exits {
        if f.exit == Exit::Normal {
            r.edge(&f, "finish");
        }
    }
    r.line("classDef k_call fill:#e8f0ff,stroke:#4b6cb7");
    r.line("classDef k_emit fill:#e5f5e0,stroke:#4a8a3b");
    r.line("classDef k_fail fill:#ffe4e4,stroke:#b33");
    r.line("classDef k_term fill:#eee,stroke:#666");
    Ok(r.output)
}
