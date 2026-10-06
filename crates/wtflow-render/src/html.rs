//! Offline flow explorer. Source text is escaped; personal notes only edit labels.
use wtflow_core::{context::FlowContext, Flow, Kind, Node};

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub fn render(flow: &Flow, report: &str) -> String {
    render_with_context(flow, report, &FlowContext::default())
}

pub fn render_with_context(flow: &Flow, report: &str, context: &FlowContext) -> String {
    let empty = FlowContext::default();
    let context = if context.fingerprint == flow.fingerprint {
        context
    } else {
        &empty
    };
    let mut nodes = Vec::new();
    wtflow_core::visit(&flow.steps, &mut nodes);
    let expanded = nodes
        .iter()
        .filter(|n| n.kind == Kind::Group && n.target.is_some())
        .count();
    let mut out = format!("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{} · wtflow</title><style>{}</style></head><body><main><header><p class=\"eyebrow\">wtflow / What the flow?</p><h1>{}</h1><p class=\"trigger\">{}</p><p class=\"muted\">{} steps · {} internal calls followed</p><p class=\"source\">{}#{}</p></header>",
        escape(&flow.flow), include_str!("html.css"), escape(&flow.flow), escape(&flow.trigger), nodes.len(), expanded,
        escape(&flow.entry.file), escape(&flow.entry.symbol));
    out.push_str(&format!("<div id=\"note-store\" data-flow=\"{}\"><button id=\"export-notes\">Export notes as labels</button><span id=\"note-status\" role=\"status\">Your notes stay in this browser. Export to keep a portable copy.</span></div>",
        escape(&format!("{}|{}|{}#{}", flow.owner, flow.flow, flow.entry.file, flow.entry.symbol))));
    out.push_str("<nav aria-label=\"Flow controls\"><button id=\"expand\">Expand all</button><button id=\"collapse\">Overview</button><a href=\"#notes\">Analysis notes</a></nav><p class=\"hint\">Read from top to bottom. Expand calls to explore; source comments explain their purpose. Orange blocks repeat.</p><div class=\"endpoint\">Start</div>");
    steps(&mut out, &flow.steps, 0, context);
    out.push_str("<div class=\"endpoint\">End of flow view</div><section id=\"notes\"><h2>Analysis notes</h2><p>Internal calls are followed where targets can be resolved. External and unresolved calls remain visible. Branches are alternatives; loops repeat; a return exits its current function. Dynamic calls and implicit exception paths may be incomplete.</p>");
    if report.is_empty() {
        out.push_str("<p>No additional notes were saved with this flow.</p>");
    } else {
        out.push_str(&format!("<pre>{}</pre>", escape(report)));
    }
    out.push_str("</section><footer>Generated locally by wtflow. No network requests.</footer></main><script>");
    out.push_str(include_str!("html.js"));
    out.push_str("</script></body></html>\n");
    out
}

fn branch(out: &mut String, title: &str, nodes: &[Node], depth: usize, context: &FlowContext) {
    out.push_str(&format!(
        "<section class=\"branch\"><h3>{}</h3>",
        escape(title)
    ));
    if nodes.is_empty() {
        out.push_str("<p class=\"muted\">No recorded steps</p>");
    } else {
        steps(out, nodes, depth, context);
    }
    out.push_str("</section>");
}

fn steps(out: &mut String, nodes: &[Node], depth: usize, context: &FlowContext) {
    out.push_str("<ol class=\"steps\">");
    for n in nodes {
        let container = matches!(
            n.kind,
            Kind::Group
                | Kind::If
                | Kind::Switch
                | Kind::ForEach
                | Kind::While
                | Kind::Parallel
                | Kind::Try
        );
        let method_call = n.kind == Kind::Call
            || (n.kind == Kind::Group && (n.target.is_some() || n.symbol.is_some()));
        let loop_node = matches!(n.kind, Kind::ForEach | Kind::While);
        let name = if method_call {
            n.target.as_deref().unwrap_or(&n.code)
        } else {
            n.label.as_deref().unwrap_or(&n.code)
        };
        let info = context.calls.get(&n.id);
        out.push_str(&format!("<li class=\"step k_{}\">", n.kind.as_str()));
        if container {
            out.push_str(&format!(
                "<details class=\"flow{}\"{}><summary>",
                if depth < 2 { " overview" } else { "" },
                if depth < 2 { " open" } else { "" }
            ));
        } else {
            out.push_str("<div class=\"heading\">");
        }
        if loop_node {
            out.push_str("<span class=\"loop-label\">↻ LOOP</span> ");
        }
        out.push_str(&format!(
            "<span class=\"kind\">{}</span> <strong>{}</strong>",
            n.kind.as_str().replace('_', " "),
            escape(name)
        ));
        if method_call {
            let ty = info
                .map(|i| i.return_type.as_str())
                .filter(|ty| !ty.is_empty())
                .unwrap_or("not available");
            out.push_str(&format!(
                "<span class=\"return-type\">Returns: {}</span>",
                escape(ty)
            ));
        }
        if let Some(info) = info {
            if !info.documentation.is_empty() {
                out.push_str(&format!("<span class=\"purpose\"><span class=\"purpose-label\">Purpose · source documentation</span>{}</span>", escape(&info.documentation.join("\n\n"))));
            }
        }
        if container {
            out.push_str("</summary>");
        } else {
            out.push_str("</div>");
        }
        if method_call || matches!(n.kind, Kind::Emit | Kind::Wait) {
            out.push_str(&format!("<details class=\"personal-note\"{}><summary>Your purpose note</summary><label>Explain why this call matters<textarea class=\"note-input\" data-id=\"{}\" data-code=\"{}\" placeholder=\"Add your own explanation…\">{}</textarea></label></details>",
                if n.label.as_ref().is_some_and(|s| !s.is_empty()) { " open" } else { "" }, escape(&n.id), escape(&n.code), escape(n.label.as_deref().unwrap_or(""))));
        }
        for (title, value) in [
            ("Boundary", n.boundary.as_deref()),
            ("Topic", n.topic.as_deref()),
            ("Transaction", n.tx.as_deref()),
        ] {
            if let Some(value) = value {
                out.push_str(&format!(
                    "<p class=\"annotation\">{title}: {}</p>",
                    escape(value)
                ));
            }
        }
        for (title, values) in [("Reads", &n.reads), ("Writes", &n.writes)] {
            if !values.is_empty() {
                out.push_str(&format!(
                    "<p class=\"annotation\">{title}: {}</p>",
                    escape(&values.join(", "))
                ));
            }
        }
        if n.kind == Kind::Call {
            out.push_str("<p class=\"muted\">Call retained as a step; see source details and analysis notes.</p>");
        }
        if n.kind == Kind::Do {
            out.push_str("<p class=\"muted\">Operation; call target was not resolved.</p>");
        }
        out.push_str(&format!("<details class=\"metadata\"><summary>Source &amp; details</summary><pre>{}</pre><p class=\"source\">{}</p><p class=\"source\">Step: {}</p>", escape(&n.code), escape(&n.src), escape(&n.id)));
        if let Some(info) = info {
            if !info.signature.is_empty() {
                out.push_str(&format!(
                    "<pre class=\"signature\">{}</pre>",
                    escape(&info.signature)
                ));
            }
            if !info.definition.is_empty() {
                out.push_str(&format!(
                    "<p class=\"source\">Definition: {}</p>",
                    escape(&info.definition)
                ));
            }
        }
        if let Some(symbol) = &n.symbol {
            out.push_str(&format!("<p class=\"source\">{}</p>", escape(symbol)));
        }
        out.push_str("</details>");
        match n.kind {
            Kind::If => {
                branch(out, "Yes", &n.then, depth + 1, context);
                branch(out, "No", &n.otherwise, depth + 1, context);
            }
            Kind::Switch => {
                for case in &n.cases {
                    let title = if case.fallthrough == Some(true) {
                        format!("{} (falls through)", case.when)
                    } else {
                        case.when.clone()
                    };
                    branch(out, &title, &case.steps, depth + 1, context);
                }
                if !n.default.is_empty() {
                    branch(out, "Default", &n.default, depth + 1, context);
                }
            }
            Kind::Try => {
                branch(out, "Try", &n.body, depth + 1, context);
                if !n.catch.is_empty() {
                    branch(out, "On error", &n.catch, depth + 1, context);
                }
                if !n.finally.is_empty() {
                    branch(out, "Finally", &n.finally, depth + 1, context);
                }
            }
            Kind::ForEach | Kind::While => branch(out, "Repeat body", &n.body, depth + 1, context),
            Kind::Parallel => branch(out, "In parallel", &n.body, depth + 1, context),
            Kind::Group => steps(out, &n.body, depth + 1, context),
            _ => {}
        }
        if container {
            out.push_str("</details>");
        }
        out.push_str("</li>");
    }
    out.push_str("</ol>");
}
