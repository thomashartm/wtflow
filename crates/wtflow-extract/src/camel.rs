use crate::{
    source::{children, normalized, SourceFile},
    Cx, Scope,
};
use tree_sitter::Node as Ast;
use wtflow_core::{Case, Kind, Node};
#[derive(Clone)]
struct Op {
    name: String,
    args: String,
    src: String,
}
pub struct Route {
    pub id: String,
    pub uri: String,
    ops: Vec<Op>,
}
fn unquote(s: &str) -> String {
    s.trim().trim_matches(['\'', '"']).into()
}
fn chain(file: &SourceFile, n: Ast<'_>, ops: &mut Vec<Op>) {
    if n.kind() == "method_invocation" {
        if let Some(object) = n.child_by_field_name("object") {
            chain(file, object, ops);
        }
        if let Some(name) = n.child_by_field_name("name") {
            let args = n
                .child_by_field_name("arguments")
                .map(|a| &file.text(a)[1..file.text(a).len() - 1])
                .unwrap_or("");
            ops.push(Op {
                name: file.text(name).into(),
                args: normalized(args),
                src: file.src(name),
            });
        }
    } else {
        for child in children(n) {
            chain(file, child, ops);
        }
    }
}
pub fn route(file: &SourceFile, n: Ast<'_>, _class: &str) -> Option<Route> {
    let mut ops = vec![];
    chain(file, n, &mut ops);
    if ops.first()?.name != "from" {
        return None;
    }
    let uri = unquote(&ops[0].args);
    let id = ops
        .iter()
        .find(|o| o.name == "routeId")
        .map(|o| unquote(&o.args))
        .unwrap_or_else(|| uri.clone());
    Some(Route { id, uri, ops })
}
pub fn extract(
    cx: &Cx,
    file: &SourceFile,
    class: &str,
    id: Option<&str>,
    scope: &mut Scope,
) -> Vec<Node> {
    let mut result = vec![];
    if let Some(f) = cx
        .funcs
        .get(&file.path)
        .into_iter()
        .flatten()
        .find(|f| f.class == class && f.name == "configure")
    {
        if let Some(body) =
            crate::functions::node(file, f).and_then(|n| n.child_by_field_name("body"))
        {
            for stmt in children(body) {
                if let Some(route) = route(file, stmt, class) {
                    if id.is_none() || id == Some(route.id.as_str()) {
                        let mut i = 0;
                        let steps = linear(cx, file, class, &route.ops, &mut i, scope, &[]);
                        if id.is_some() {
                            result.extend(steps);
                        } else {
                            let mut group =
                                Node::new(Kind::Group, format!("from(\"{}\")", route.uri));
                            group.src = file.src(stmt);
                            group.target = Some(format!("{class}@{}", route.id));
                            group.body = steps;
                            result.push(group);
                        }
                    }
                }
            }
        }
    }
    result
}
fn linear(
    cx: &Cx,
    file: &SourceFile,
    class: &str,
    ops: &[Op],
    i: &mut usize,
    scope: &mut Scope,
    stops: &[&str],
) -> Vec<Node> {
    let mut out = vec![];
    while let Some(op) = ops.get(*i) {
        if stops.contains(&op.name.as_str()) {
            break;
        }
        *i += 1;
        if matches!(op.name.as_str(), "from" | "routeId" | "log") {
            continue;
        }
        if op.name == "end" {
            break;
        }
        let mut n = Node::new(Kind::Do, format!("{}({})", op.name, op.args));
        n.src = op.src.clone();
        match op.name.as_str() {
            "choice" => {
                n.kind = Kind::Switch;
                n.code = "choice".into();
                while let Some(branch) = ops.get(*i) {
                    if branch.name == "end" {
                        *i += 1;
                        break;
                    }
                    if branch.name == "when" {
                        *i += 1;
                        let steps = linear(
                            cx,
                            file,
                            class,
                            ops,
                            i,
                            scope,
                            &["when", "otherwise", "end"],
                        );
                        n.cases.push(Case {
                            when: branch.args.clone(),
                            fallthrough: None,
                            steps,
                        });
                    } else if branch.name == "otherwise" {
                        *i += 1;
                        n.default = linear(cx, file, class, ops, i, scope, &["end"]);
                    } else {
                        *i += 1;
                    }
                }
            }
            "filter" | "split" | "loop" | "multicast" | "transacted" => {
                n.kind = match op.name.as_str() {
                    "filter" => Kind::If,
                    "split" => Kind::ForEach,
                    "loop" => Kind::While,
                    "multicast" => Kind::Parallel,
                    _ => Kind::Group,
                };
                if op.name == "transacted" {
                    n.tx = Some("db".into());
                }
                let body = linear(cx, file, class, ops, i, scope, &[]);
                if n.kind == Kind::If {
                    n.then = body;
                } else {
                    n.body = body;
                }
            }
            "doTry" => {
                n.kind = Kind::Try;
                n.body = linear(
                    cx,
                    file,
                    class,
                    ops,
                    i,
                    scope,
                    &["doCatch", "doFinally", "end"],
                );
                let mut cases = vec![];
                while let Some(handler) = ops.get(*i) {
                    if handler.name != "doCatch" {
                        break;
                    }
                    *i += 1;
                    let steps = linear(
                        cx,
                        file,
                        class,
                        ops,
                        i,
                        scope,
                        &["doCatch", "doFinally", "end"],
                    );
                    cases.push(Case {
                        when: handler.args.clone(),
                        fallthrough: None,
                        steps,
                    });
                }
                if cases.len() == 1 {
                    n.catch = cases.remove(0).steps;
                } else if !cases.is_empty() {
                    let mut dispatch = Node::new(Kind::Switch, "error type");
                    dispatch.src = n.src.clone();
                    dispatch.cases = cases;
                    n.catch.push(dispatch);
                }
                if ops.get(*i).is_some_and(|o| o.name == "doFinally") {
                    *i += 1;
                    n.finally = linear(cx, file, class, ops, i, scope, &["end"]);
                }
                if ops.get(*i).is_some_and(|o| o.name == "end") {
                    *i += 1;
                }
            }
            "to" | "toD" | "wireTap" => {
                let uri = unquote(&op.args);
                if uri.starts_with("direct:") || uri.starts_with("seda:") {
                    let id = uri.split_once(':').map(|(_, id)| id).unwrap_or(&uri);
                    let key = format!("{class}@{id}");
                    n.kind = Kind::Call;
                    n.target = Some(key.clone());
                    if scope.depth < scope.max_depth && !scope.path.contains(&key) {
                        scope.path.push(key);
                        scope.depth += 1;
                        n.kind = Kind::Group;
                        n.body = extract(cx, file, class, Some(id), scope);
                        scope.depth -= 1;
                        scope.path.pop();
                    }
                } else if ["google-pubsub:", "kafka:", "jms:", "amqp:"]
                    .iter()
                    .any(|p| uri.starts_with(p))
                {
                    n.kind = Kind::Emit;
                    n.topic = Some(
                        uri.split('?')
                            .next()
                            .unwrap_or(&uri)
                            .rsplit(':')
                            .next()
                            .unwrap_or(&uri)
                            .into(),
                    );
                } else if uri.starts_with("jpa:") || uri.starts_with("sql:") {
                    n.kind = Kind::Call;
                    n.writes
                        .push(uri.split_once(':').map(|(_, s)| s).unwrap_or("db").into());
                } else if uri.starts_with("http:") || uri.starts_with("https:") {
                    n.kind = Kind::Call;
                    n.boundary = Some(
                        uri.split("//")
                            .nth(1)
                            .unwrap_or(&uri)
                            .split('/')
                            .next()
                            .unwrap_or(&uri)
                            .into(),
                    );
                }
            }
            _ => {}
        }
        out.push(n);
    }
    out
}
