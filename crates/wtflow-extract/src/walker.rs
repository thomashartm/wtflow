use crate::{
    functions,
    source::{children, normalized, SourceFile},
    Cx, Scope,
};
use tree_sitter::Node as Ast;
use wtflow_core::{Case, Kind, Node};
use wtflow_resolve::{ByteRange, Resolution};
pub struct Walker<'a> {
    pub cx: &'a Cx,
    pub file: &'a SourceFile,
    pub scope: &'a mut Scope,
}
impl Walker<'_> {
    fn node(&self, kind: Kind, code: impl Into<String>, ast: Ast<'_>) -> Node {
        let mut n = Node::new(kind, code);
        n.src = self.file.src(ast);
        n
    }
    fn text(&self, n: Ast<'_>) -> String {
        normalized(self.file.text(n))
    }
    fn field(&self, n: Ast<'_>, field: &str) -> String {
        n.child_by_field_name(field)
            .map(|n| self.text(n))
            .unwrap_or_default()
    }
    fn block(&mut self, n: Option<Ast<'_>>) -> Vec<Node> {
        n.map(|n| self.walk(n)).unwrap_or_default()
    }
    pub fn walk(&mut self, n: Ast<'_>) -> Vec<Node> {
        match n.kind() {
            "statement_block" | "block" | "else_clause" | "finally_clause" => {
                let mut out = vec![];
                for c in children(n) {
                    out.extend(self.walk(c));
                }
                self.collapse(out)
            }
            "if_statement" | "elif_clause" => {
                let cond = self.field(n, "condition");
                let mut node = self.node(Kind::If, strip_parens(&cond), n);
                node.then = self.block(
                    n.child_by_field_name("consequence")
                        .or_else(|| n.child_by_field_name("body")),
                );
                let alternatives: Vec<_> = children(n)
                    .into_iter()
                    .filter(|c| matches!(c.kind(), "elif_clause" | "else_clause"))
                    .collect();
                if !alternatives.is_empty() {
                    let mut tail = vec![];
                    for alt in alternatives.into_iter().rev() {
                        if alt.kind() == "elif_clause" {
                            let mut branch = self.walk(alt);
                            if let Some(first) = branch.first_mut() {
                                first.otherwise = tail;
                            }
                            tail = branch;
                        } else {
                            tail = self.walk(alt);
                        }
                    }
                    node.otherwise = tail;
                } else {
                    node.otherwise = self.block(n.child_by_field_name("alternative"));
                }
                vec![node]
            }
            "for_in_statement" | "for_statement" | "enhanced_for_statement" => {
                let code = self.file.text(n);
                let body = n.child_by_field_name("body");
                let end = body
                    .map(|b| b.start_byte() - n.start_byte())
                    .unwrap_or(code.len());
                let header = code[..end].trim();
                let inside = header
                    .strip_prefix("async ")
                    .unwrap_or(header)
                    .trim_start_matches("for")
                    .trim();
                let inside = strip_parens(inside.trim_end_matches(':').trim());
                let is_c =
                    n.kind() == "for_statement" && self.file.lang != crate::source::Language::Py;
                let unconditional = is_c && self.field(n, "condition").is_empty();
                let kind = if unconditional {
                    Kind::While
                } else {
                    Kind::ForEach
                };
                let code = if unconditional {
                    "for(;;)".into()
                } else {
                    inside
                };
                let mut node = self.node(kind, code, n);
                node.body = self.block(body);
                vec![node]
            }
            "while_statement" | "do_statement" => {
                let cond = self.field(n, "condition");
                let mut node = self.node(Kind::While, strip_parens(&cond), n);
                node.body = self.block(n.child_by_field_name("body"));
                vec![node]
            }
            "return_statement" | "throw_statement" | "raise_statement" | "break_statement"
            | "continue_statement" => {
                let kind = match n.kind() {
                    "return_statement" => Kind::Return,
                    "break_statement" => Kind::Break,
                    "continue_statement" => Kind::Continue,
                    _ => Kind::Fail,
                };
                let mut steps = if kind == Kind::Return {
                    children(n)
                        .into_iter()
                        .find_map(outer_call)
                        .map(|call| self.call(call))
                        .unwrap_or_default()
                } else {
                    vec![]
                };
                steps.push(self.node(kind, self.text(n), n));
                steps
            }
            "switch_statement" | "switch_expression" | "match_statement" => self.switch(n),
            "try_statement" | "try_with_resources_statement" => {
                let mut node = self.node(Kind::Try, "try", n);
                node.body = self.block(n.child_by_field_name("body"));
                let handlers: Vec<_> = children(n)
                    .into_iter()
                    .filter(|c| matches!(c.kind(), "catch_clause" | "except_clause"))
                    .collect();
                if handlers.len() > 1 {
                    let mut dispatch = self.node(Kind::Switch, "error type", n);
                    for handler in handlers {
                        let body = handler.child_by_field_name("body").or_else(|| {
                            children(handler).into_iter().find(|c| c.kind() == "block")
                        });
                        let when = children(handler)
                            .into_iter()
                            .find(|c| Some(*c) != body)
                            .map(|c| self.text(c))
                            .unwrap_or_else(|| "Exception".into());
                        let mut steps = self.block(body);
                        if steps.is_empty() {
                            steps.push(self.node(Kind::Do, "(error ignored)", handler));
                        }
                        dispatch.cases.push(Case {
                            when,
                            fallthrough: None,
                            steps,
                        });
                    }
                    node.catch.push(dispatch);
                } else if let Some(handler) = handlers.first() {
                    node.catch = self.block(handler.child_by_field_name("body").or_else(|| {
                        children(*handler)
                            .into_iter()
                            .find(|c| matches!(c.kind(), "block" | "statement_block"))
                    }));
                    if node.catch.is_empty() {
                        node.catch
                            .push(self.node(Kind::Do, "(error ignored)", *handler));
                    }
                }
                node.finally = self.block(
                    children(n)
                        .into_iter()
                        .find(|c| c.kind() == "finally_clause"),
                );
                vec![node]
            }
            "with_statement" => {
                let mut node = self.node(
                    Kind::Group,
                    self.file.text(n).split(':').next().unwrap_or("with"),
                    n,
                );
                node.tx = Some("db".into());
                node.body = self.block(n.child_by_field_name("body"));
                vec![node]
            }
            "function_declaration" | "function_definition" | "class_declaration" | "comment" => {
                vec![]
            }
            _ => {
                if let Some(call) = outer_call(n) {
                    self.call(call)
                } else {
                    vec![]
                }
            }
        }
    }
    fn collapse(&self, nodes: Vec<Node>) -> Vec<Node> {
        if !self.cx.config.config.collapse {
            return nodes;
        }
        let mut out: Vec<Node> = vec![];
        for n in nodes {
            if n.kind == Kind::Do && n.reads.is_empty() && n.writes.is_empty() && n.tx.is_none() {
                if let Some(prev) = out.last_mut().filter(|p| {
                    p.kind == Kind::Do
                        && p.reads.is_empty()
                        && p.writes.is_empty()
                        && p.tx.is_none()
                }) {
                    prev.code.push('\n');
                    prev.code.push_str(&n.code);
                    if let (Some((file, start)), Some((_, end))) =
                        (prev.src.rsplit_once(':'), n.src.rsplit_once(':'))
                    {
                        prev.src = format!(
                            "{}:{}-{}",
                            file,
                            start.split('-').next().unwrap_or(start),
                            end.rsplit('-').next().unwrap_or(end)
                        );
                    }
                    continue;
                }
            }
            out.push(n);
        }
        out
    }
    fn switch(&mut self, n: Ast<'_>) -> Vec<Node> {
        let cond = n
            .child_by_field_name("value")
            .or_else(|| n.child_by_field_name("condition"))
            .or_else(|| n.child_by_field_name("subject"));
        let mut node = self.node(
            Kind::Switch,
            cond.map(|c| strip_parens(&self.text(c)))
                .unwrap_or_default(),
            n,
        );
        let body = n.child_by_field_name("body").unwrap_or(n);
        for case in children(body).into_iter().filter(|c| {
            matches!(
                c.kind(),
                "switch_case"
                    | "switch_default"
                    | "case_clause"
                    | "switch_block_statement_group"
                    | "switch_rule"
            )
        }) {
            let parts = children(case);
            let value = case.child_by_field_name("value").or_else(|| {
                parts
                    .first()
                    .copied()
                    .filter(|c| matches!(c.kind(), "case_pattern" | "switch_label"))
            });
            let when = value
                .map(|v| self.text(v).trim_start_matches("case ").to_owned())
                .unwrap_or_default();
            let mut steps = vec![];
            for child in parts {
                if Some(child) != value {
                    steps.extend(self.walk(child));
                }
            }
            let default = case.kind() == "switch_default" || when == "_" || when == "default";
            if default {
                node.default = steps;
            } else {
                let fallthrough = if self.file.lang == crate::source::Language::Py
                    || case.kind() == "switch_rule"
                {
                    None
                } else {
                    Some(!steps.last().is_some_and(|s| s.kind.terminal()))
                };
                node.cases.push(Case {
                    when,
                    fallthrough,
                    steps,
                });
            }
        }
        vec![node]
    }
    pub fn call(&mut self, call: Ast<'_>) -> Vec<Node> {
        let code = self.text(call);
        if self.cx.config.ignore.iter().any(|r| r.is_match(&code)) {
            return vec![];
        }
        let callee = functions::callee(call);
        let (resolution, from_scip) = callee
            .map(|c| {
                self.cx.chain.resolve_with_origin(
                    &self.file.path,
                    ByteRange {
                        start: c.start_byte(),
                        end: c.end_byte(),
                    },
                )
            })
            .unwrap_or((Resolution::Unresolved, false));
        if !from_scip && !matches!(resolution, Resolution::Unresolved) {
            self.scope.heuristic_used = true;
        }
        let symbol = match &resolution {
            Resolution::Def { symbol, .. }
            | Resolution::Impls { symbol, .. }
            | Resolution::External { symbol, .. } => Some(symbol.as_str()),
            Resolution::Unresolved => None,
        }
        .filter(|_| from_scip);
        for rule in &self.cx.config.rules {
            let captures = rule.pattern.as_ref().and_then(|r| r.captures(&code));
            if rule.pattern.is_some() && captures.is_none() {
                continue;
            }
            if rule
                .symbol
                .as_ref()
                .is_some_and(|r| !symbol.is_some_and(|s| r.is_match(s)))
            {
                continue;
            }
            let expand = |v: &Option<String>| {
                v.as_ref().map(|v| {
                    if let Some(c) = &captures {
                        let mut out = String::new();
                        c.expand(v, &mut out);
                        out
                    } else {
                        v.clone()
                    }
                })
            };
            let mut node = self.node(
                rule.rule.kind.unwrap_or(if rule.rule.inline_callback {
                    Kind::Group
                } else {
                    Kind::Call
                }),
                code.clone(),
                call,
            );
            node.boundary = expand(&rule.rule.boundary);
            node.topic = expand(&rule.rule.topic);
            node.target = expand(&rule.rule.target);
            node.tx = expand(&rule.rule.tx);
            if from_scip {
                node.symbol = symbol.map(str::to_owned);
            }
            node.reads = expand(&rule.rule.reads).into_iter().collect();
            node.writes = expand(&rule.rule.writes).into_iter().collect();
            if rule.rule.inline_callback {
                if let Some(cb) = callback(call) {
                    node.body = self.block(cb.child_by_field_name("body"));
                }
            }
            return vec![node];
        }
        let callee_text = functions::callee_text(self.file, call);
        if callee_text.ends_with(".forEach") {
            let mut node = self.node(
                Kind::ForEach,
                callee_text.trim_end_matches(".forEach"),
                call,
            );
            if let Some(cb) = callback(call) {
                node.body = self.block(cb.child_by_field_name("body"));
            }
            return vec![node];
        }
        if matches!(
            callee_text,
            "Promise.all" | "Promise.allSettled" | "asyncio.gather"
        ) {
            let mut node = self.node(Kind::Parallel, callee_text, call);
            if let Some(args) = call.child_by_field_name("arguments") {
                for arg in children(args) {
                    let items = if matches!(arg.kind(), "array" | "list") {
                        children(arg)
                    } else {
                        vec![arg]
                    };
                    for item in items {
                        if let Some(call) = outer_call(item) {
                            node.body.extend(self.call(call));
                        }
                    }
                }
            }
            return vec![node];
        }
        match resolution {
            Resolution::Def {
                symbol,
                file,
                range,
            } => vec![self.resolved(call, code, &symbol, &file, range, from_scip)],
            Resolution::External { symbol, package } => {
                let mut n = self.node(Kind::Call, code, call);
                n.boundary = Some(package);
                if from_scip {
                    n.symbol = Some(symbol);
                }
                vec![n]
            }
            Resolution::Impls { symbol, mut impls } => {
                impls.sort_by(|a, b| a.symbol.cmp(&b.symbol));
                if impls.len() == 1 {
                    let d = &impls[0];
                    return vec![self.resolved(call, code, &d.symbol, &d.file, d.range, from_scip)];
                }
                let mut n = self.node(Kind::Switch, format!("dispatch {symbol}"), call);
                for d in impls {
                    let child =
                        self.resolved(call, code.clone(), &d.symbol, &d.file, d.range, from_scip);
                    n.cases.push(Case {
                        when: d.symbol,
                        fallthrough: None,
                        steps: vec![child],
                    });
                }
                vec![n]
            }
            Resolution::Unresolved => vec![self.node(Kind::Do, code, call)],
        }
    }
    fn resolved(
        &mut self,
        call: Ast<'_>,
        code: String,
        symbol: &str,
        file: &str,
        range: ByteRange,
        from_scip: bool,
    ) -> Node {
        let func = self
            .cx
            .funcs
            .get(file)
            .into_iter()
            .flatten()
            .filter(|f| f.range.start <= range.start && f.range.end >= range.end)
            .min_by_key(|f| f.range.end - f.range.start);
        let mut n = self.node(Kind::Call, code, call);
        if from_scip {
            n.symbol = Some(symbol.into());
        }
        n.target = Some(func.map(|f| f.symbol()).unwrap_or_else(|| symbol.into()));
        let owner = self.cx.config.owner(file);
        if !owner.is_empty() && owner != self.scope.owner {
            n.boundary = Some(owner);
        }
        let key = format!(
            "{file}#{}",
            func.map(|f| f.symbol()).unwrap_or_else(|| symbol.into())
        );
        if let Some(f) = func {
            if self.scope.depth < self.scope.max_depth && !self.scope.path.contains(&key) {
                self.scope.path.push(key);
                self.scope.depth += 1;
                n.kind = Kind::Group;
                n.body = self.cx.body(f, self.scope);
                self.scope.depth -= 1;
                self.scope.path.pop();
            }
        }
        n
    }
}
pub fn strip_parens(s: &str) -> String {
    if s.starts_with('(') && s.ends_with(')') {
        s[1..s.len() - 1].trim().into()
    } else {
        s.trim().into()
    }
}
pub fn outer_call(n: Ast<'_>) -> Option<Ast<'_>> {
    if functions::is_call(n) {
        return Some(n);
    }
    if matches!(
        n.kind(),
        "arrow_function" | "lambda" | "function_expression"
    ) {
        return None;
    }
    children(n).into_iter().find_map(outer_call)
}
fn callback(n: Ast<'_>) -> Option<Ast<'_>> {
    let args = n.child_by_field_name("arguments")?;
    children(args).into_iter().find(|n| {
        matches!(
            n.kind(),
            "arrow_function" | "function_expression" | "lambda"
        )
    })
}
