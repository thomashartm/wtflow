# Proposed labels.cache contract

This is a design note. wtflow does not read/write a label cache and makes no LLM
calls. An external labeling client can consume `wtflow todo --json --context`
and return a flat mapping to `wtflow label`; wtflow retains authority over flow
structure and verifies its fingerprint before applying any labels.

The cache key is SHA-256 over compact UTF-8 JSON with these two ordered fields:

```
{"node":<fingerprint-relevant node object>,"glossary_version":<version string>}
```

Construct `node` using the v1 structural reduction and exact field order in
`wtflow-core::fingerprint`, including child structure. Exclude label and src.
Use the full lowercase hexadecimal SHA-256 digest for the cache key, not the
flow document's eight-byte abbreviation. The value is a label string. If there
is no glossary, use an empty glossary-version string. The glossary owner must
change its version whenever terminology changes. A client must not reuse cached
labels across glossary versions or treat cache contents as structural edits.

A possible on-disk representation is a JSON object of key-to-label entries,
written with lexicographically sorted keys and atomic replacement. Cache writes,
eviction, model/provider identity, and conflict resolution remain outside this
implementation. A cache hit still goes through `wtflow label` and its ID and
fingerprint checks. Clients needing context-sensitive labels can decline a cache
hit; the specified structural key deliberately does not hash neighboring labels.

Context packets include the full node, its document path, ancestor node IDs
(root to parent), previous/next sibling IDs, and available callee signature and
documentation. Signatures come from SCIP signature documentation or fenced code
in SymbolInformation.documentation, with an AST signature fallback when SCIP
provides only prose. Unresolved or stale callee metadata is represented by null.

An optional `glossary.yaml` in the `.wtflow.yaml` repository root is parsed as YAML
and passed through as JSON in each packet. Its format is intentionally open; for
example, `version: '1'` and `terms: {openItem: outstanding invoice}`. Glossary data
is context only and never changes structural output or the flow fingerprint.
