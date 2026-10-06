//! Field order here is part of the v1 wire contract; never use a JSON map.
use crate::{Case, Kind, Node};
use serde::Serialize;
use sha2::{Digest, Sha256};
#[derive(Serialize)]
struct Structural<'a> {
    id: &'a str,
    kind: Kind,
    #[serde(skip_serializing_if = "str::is_empty")]
    code: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    symbol: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    boundary: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    topic: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tx: Option<&'a str>,
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    reads: &'a [String],
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    writes: &'a [String],
    #[serde(skip_serializing_if = "Vec::is_empty")]
    then: Vec<Structural<'a>>,
    #[serde(rename = "else", skip_serializing_if = "Vec::is_empty")]
    otherwise: Vec<Structural<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    cases: Vec<StructuralCase<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    default: Vec<Structural<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    body: Vec<Structural<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    catch: Vec<Structural<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    finally: Vec<Structural<'a>>,
}
#[derive(Serialize)]
struct StructuralCase<'a> {
    when: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    fallthrough: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    steps: Vec<Structural<'a>>,
}
fn case(c: &Case) -> StructuralCase<'_> {
    StructuralCase {
        when: &c.when,
        fallthrough: c.fallthrough,
        steps: reduced(&c.steps),
    }
}
fn reduced(nodes: &[Node]) -> Vec<Structural<'_>> {
    nodes
        .iter()
        .map(|n| Structural {
            id: &n.id,
            kind: n.kind,
            code: &n.code,
            target: n.target.as_deref().filter(|v| !v.is_empty()),
            symbol: n.symbol.as_deref().filter(|v| !v.is_empty()),
            boundary: n.boundary.as_deref().filter(|v| !v.is_empty()),
            topic: n.topic.as_deref().filter(|v| !v.is_empty()),
            tx: n.tx.as_deref().filter(|v| !v.is_empty()),
            reads: &n.reads,
            writes: &n.writes,
            then: reduced(&n.then),
            otherwise: reduced(&n.otherwise),
            cases: n.cases.iter().map(case).collect(),
            default: reduced(&n.default),
            body: reduced(&n.body),
            catch: reduced(&n.catch),
            finally: reduced(&n.finally),
        })
        .collect()
}
pub fn canonical_json(nodes: &[Node]) -> anyhow::Result<Vec<u8>> {
    Ok(serde_json::to_vec(&reduced(nodes))?)
}
pub fn compute(nodes: &[Node]) -> anyhow::Result<String> {
    let digest = Sha256::digest(canonical_json(nodes)?);
    Ok(hex::encode(&digest[..8]))
}
