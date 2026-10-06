use crate::{
    functions::Func,
    source::{children, SourceFile},
    Cx,
};
use serde::Serialize;
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EntryPoint {
    pub file: String,
    pub symbol: String,
    pub trigger: String,
    pub lang: String,
}
fn argument(s: &str) -> String {
    let s = s.split_once('(').map(|(_, s)| s).unwrap_or("").trim();
    if let Some(q @ ('\'' | '"' | '`')) = s.chars().next() {
        return s[1..].split(q).next().unwrap_or("").into();
    }
    s.split(')').next().unwrap_or("").trim().into()
}
pub fn trigger(file: &SourceFile, f: &Func) -> String {
    let mut annotations = f.annotations.clone();
    if let Some(n) = crate::functions::node(file, f) {
        let mut previous = n.prev_named_sibling();
        let mut decorators = vec![];
        while let Some(p) = previous {
            if p.kind() != "decorator" {
                break;
            }
            decorators.push(file.text(p));
            previous = p.prev_named_sibling();
        }
        decorators.reverse();
        annotations = format!("{} {}", decorators.join(" "), annotations);
    }
    let mut prefix = String::new();
    let before = &file.text[..f.range.start];
    for key in ["@Controller(", "@Path("] {
        if let Some((_, tail)) = before.rsplit_once(key) {
            prefix = argument(&format!("({tail}"));
            break;
        }
    }
    for raw in annotations.split('@').skip(1) {
        let name = raw.split(['(', ' ', '\n']).next().unwrap_or("");
        let short = name.rsplit('.').next().unwrap_or(name);
        let arg = argument(raw);
        let verb = match short {
            "Get" | "GET" | "get" | "GetMapping" => Some("GET"),
            "Post" | "POST" | "post" | "PostMapping" => Some("POST"),
            "Put" | "PUT" | "put" | "PutMapping" => Some("PUT"),
            "Delete" | "DELETE" | "delete" | "DeleteMapping" => Some("DELETE"),
            "Patch" | "PATCH" | "patch" | "PatchMapping" => Some("PATCH"),
            "Head" | "HEAD" | "head" => Some("HEAD"),
            "Options" | "OPTIONS" | "options" => Some("OPTIONS"),
            "route" | "RequestMapping" => Some("GET"),
            _ => None,
        };
        if let Some(verb) = verb {
            let path = format!("/{}/{}", prefix.trim_matches('/'), arg.trim_matches('/'))
                .replace("//", "/");
            return format!(
                "http {verb} {}",
                if path.len() > 1 {
                    path.trim_end_matches('/')
                } else {
                    &path
                }
            );
        }
        let kind = match short {
            "EventPattern" | "MessagePattern" | "OnEvent" | "SubscribeMessage" | "subscribe"
            | "on_event" | "Incoming" | "ConsumeEvent" | "KafkaListener" | "JmsListener"
            | "RabbitListener" => "event",
            "Cron" | "Interval" | "Timeout" | "Scheduled" | "cron" => "schedule",
            "Process" | "task" | "shared_task" | "actor" => "task",
            "Query" | "Mutation" | "ResolveField" => "task",
            _ => continue,
        };
        return format!(
            "{kind} {}",
            if arg.is_empty() {
                f.name.as_str()
            } else {
                &arg
            }
        );
    }
    String::new()
}
pub fn detect(cx: &Cx, file: &SourceFile) -> Vec<EntryPoint> {
    let mut result = vec![];
    if let Some(funcs) = cx.funcs.get(&file.path) {
        for f in funcs {
            let trigger = trigger(file, f);
            if !trigger.is_empty() {
                result.push(EntryPoint {
                    file: file.path.clone(),
                    symbol: f.symbol(),
                    trigger,
                    lang: file.lang.name().into(),
                });
            }
            if f.name == "configure" {
                if let Some(n) =
                    crate::functions::node(file, f).and_then(|n| n.child_by_field_name("body"))
                {
                    for stmt in children(n) {
                        if let Some(route) = crate::camel::route(file, stmt, &f.class) {
                            result.push(EntryPoint {
                                file: file.path.clone(),
                                symbol: format!("{}@{}", f.class, route.id),
                                trigger: format!("camel {}", route.uri),
                                lang: "java".into(),
                            });
                        }
                    }
                }
            }
        }
    }
    result
}
