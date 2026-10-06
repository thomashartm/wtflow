use crate::source::{children, descendants, Language, SourceFile};
use tree_sitter::Node;
use wtflow_core::Input;
use wtflow_resolve::ByteRange;
#[derive(Clone, Debug)]
pub struct Func {
    pub file: String,
    pub name: String,
    pub class: String,
    pub range: ByteRange,
    pub inputs: Vec<Input>,
    pub output: String,
    pub annotations: String,
}
impl Func {
    pub fn symbol(&self) -> String {
        if self.class.is_empty() {
            self.name.clone()
        } else {
            format!("{}.{}", self.class, self.name)
        }
    }
}
pub fn node<'a>(file: &'a SourceFile, func: &Func) -> Option<Node<'a>> {
    file.tree
        .root_node()
        .descendant_for_byte_range(func.range.start, func.range.end)
}
pub fn collect(file: &SourceFile) -> Vec<Func> {
    fn walk(file: &SourceFile, n: Node<'_>, class: &str, out: &mut Vec<Func>) {
        let mut class = class.to_owned();
        if matches!(
            n.kind(),
            "class_declaration" | "class_definition" | "interface_declaration"
        ) {
            if let Some(name) = n.child_by_field_name("name") {
                class = file.text(name).into();
            }
        }
        if matches!(
            n.kind(),
            "function_declaration"
                | "function_definition"
                | "method_definition"
                | "method_declaration"
                | "constructor_declaration"
        ) && n.child_by_field_name("body").is_some()
        {
            if let Some(name) = n.child_by_field_name("name") {
                let mut inputs = vec![];
                if let Some(params) = n.child_by_field_name("parameters") {
                    for p in children(params) {
                        let pn = p
                            .child_by_field_name("name")
                            .or_else(|| p.child_by_field_name("pattern"))
                            .or_else(|| children(p).into_iter().find(|c| c.kind() == "identifier"))
                            .or_else(|| (p.kind() == "identifier").then_some(p));
                        if let Some(pn) = pn {
                            let name = file.text(pn);
                            if matches!(name, "self" | "cls") {
                                continue;
                            }
                            let ty = p
                                .child_by_field_name("type")
                                .map(|t| file.text(t).trim_start_matches(':').trim().to_owned())
                                .unwrap_or_default();
                            inputs.push(Input {
                                name: name.into(),
                                ty,
                            });
                        }
                    }
                }
                let output = n
                    .child_by_field_name("return_type")
                    .or_else(|| {
                        if file.lang == Language::Java {
                            n.child_by_field_name("type")
                        } else {
                            None
                        }
                    })
                    .map(|t| file.text(t).trim_start_matches(':').trim().to_owned())
                    .unwrap_or_default();
                let prefix = &file.text[n.start_byte()
                    ..n.child_by_field_name("body")
                        .map(|b| b.start_byte())
                        .unwrap_or(n.end_byte())];
                let annotations = if n
                    .parent()
                    .is_some_and(|p| p.kind() == "decorated_definition")
                {
                    n.parent()
                        .map(|p| file.text(p).split("def ").next().unwrap_or("").into())
                        .unwrap_or_default()
                } else {
                    prefix.into()
                };
                out.push(Func {
                    file: file.path.clone(),
                    name: file.text(name).into(),
                    class: class.clone(),
                    range: ByteRange {
                        start: n.start_byte(),
                        end: n.end_byte(),
                    },
                    inputs,
                    output,
                    annotations,
                });
            }
        }
        for c in children(n) {
            walk(file, c, &class, out);
        }
    }
    let mut out = vec![];
    walk(file, file.tree.root_node(), "", &mut out);
    out
}
pub fn all_calls(file: &SourceFile) -> Vec<Node<'_>> {
    let mut nodes = vec![];
    descendants(file.tree.root_node(), &mut nodes);
    nodes.into_iter().filter(|n| is_call(*n)).collect()
}
pub fn is_call(n: Node<'_>) -> bool {
    matches!(
        n.kind(),
        "call_expression" | "call" | "method_invocation" | "object_creation_expression"
    )
}
pub fn callee(call: Node<'_>) -> Option<Node<'_>> {
    if call.kind() == "method_invocation" {
        return call.child_by_field_name("name");
    }
    let n = call.child_by_field_name("function")?;
    n.child_by_field_name("property")
        .or_else(|| n.child_by_field_name("attribute"))
        .or(Some(n))
}
pub fn callee_text<'a>(file: &'a SourceFile, call: Node<'_>) -> &'a str {
    if call.kind() == "method_invocation" {
        return file.text(call).split('(').next().unwrap_or("");
    }
    call.child_by_field_name("function")
        .map(|n| file.text(n))
        .unwrap_or("")
}
