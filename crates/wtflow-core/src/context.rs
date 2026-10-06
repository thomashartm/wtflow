//! Display metadata stored alongside a flow; never part of its structure or fingerprint.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CallDetails {
    pub signature: String,
    pub return_type: String,
    pub documentation: Vec<String>,
    pub definition: String,
}

#[derive(Default, Serialize, Deserialize)]
pub struct FlowContext {
    pub fingerprint: String,
    pub calls: BTreeMap<String, CallDetails>,
}
