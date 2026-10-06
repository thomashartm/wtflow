use crate::{ByteRange, Def, RelPath, Resolution, Resolver};
use anyhow::{Context, Result};
use protobuf::Message;
use scip::types::{Document, Metadata, Occurrence, SymbolInformation};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, OnceLock},
};

/// Minimal checked wire framing lets documents remain undecoded until lookup.
struct Field<'a> {
    number: u64,
    int: u64,
    bytes: &'a [u8],
}
fn varint(bytes: &[u8], at: &mut usize) -> Result<u64> {
    let mut result = 0;
    for shift in (0..70).step_by(7) {
        let b = *bytes.get(*at).context("truncated protobuf varint")?;
        *at += 1;
        anyhow::ensure!(shift < 63 || b <= 1, "protobuf varint overflow");
        result |= u64::from(b & 127) << shift;
        if b < 128 {
            return Ok(result);
        }
    }
    anyhow::bail!("invalid protobuf varint")
}
fn fields(bytes: &[u8]) -> Result<Vec<Field<'_>>> {
    let mut at = 0;
    let mut out = vec![];
    while at < bytes.len() {
        let tag = varint(bytes, &mut at)?;
        anyhow::ensure!(tag >> 3 != 0, "invalid protobuf field zero");
        let mut field = Field {
            number: tag >> 3,
            int: 0,
            bytes: &[],
        };
        let size = match tag & 7 {
            0 => {
                field.int = varint(bytes, &mut at)?;
                0
            }
            1 => 8,
            2 => usize::try_from(varint(bytes, &mut at)?)?,
            5 => 4,
            _ => anyhow::bail!("unsupported protobuf wire type"),
        };
        let end = at.checked_add(size).context("protobuf length overflow")?;
        field.bytes = bytes.get(at..end).context("truncated protobuf field")?;
        at = end;
        out.push(field);
    }
    Ok(out)
}
struct Source {
    text: Arc<str>,
    lines: Vec<usize>,
}
// Newer official indexers use typed ranges. Decode their standard wire fields
// while retaining the Rust-1.80-compatible SCIP bindings.
fn occurrence_range(occurrence: &Occurrence) -> Result<Vec<i32>> {
    for (field, len) in [(8, 3), (9, 4)] {
        if let Some(protobuf::UnknownValueRef::LengthDelimited(bytes)) =
            occurrence.special_fields.unknown_fields().get(field)
        {
            let mut range = vec![0; len];
            for value in fields(bytes)? {
                if (1..=len as u64).contains(&value.number) {
                    range[value.number as usize - 1] = i32::try_from(value.int)?;
                }
            }
            return Ok(range);
        }
    }
    Ok(occurrence.range.clone())
}
impl Source {
    fn new(text: Arc<str>) -> Self {
        let mut lines = vec![0];
        lines.extend(
            text.bytes()
                .enumerate()
                .filter_map(|(i, b)| (b == b'\n').then_some(i + 1)),
        );
        Self { text, lines }
    }
    fn offset(&self, line: i32, column: i32, encoding: u64) -> Result<usize> {
        let line = usize::try_from(line)?;
        let column = usize::try_from(column)?;
        let start = *self.lines.get(line).context("SCIP line outside source")?;
        let end = self.lines.get(line + 1).copied().unwrap_or(self.text.len());
        let text = &self.text[start..end];
        if encoding == 1 {
            anyhow::ensure!(
                column <= text.len() && text.is_char_boundary(column),
                "SCIP UTF-8 column outside character boundary"
            );
            return Ok(start + column);
        }
        let mut units = 0;
        for (byte, ch) in text.char_indices() {
            if units == column {
                return Ok(start + byte);
            }
            units += if encoding == 2 { ch.len_utf16() } else { 1 };
        }
        anyhow::ensure!(
            units == column,
            "SCIP column outside source or inside surrogate pair"
        );
        Ok(end)
    }
    fn range(&self, range: &[i32], encoding: u64) -> Result<ByteRange> {
        let (line, start, end_line, end) = match range {
            [l, s, e] => (*l, *s, *l, *e),
            [l, s, el, e] => (*l, *s, *el, *e),
            _ => anyhow::bail!("invalid SCIP range length"),
        };
        let range = ByteRange {
            start: self.offset(line, start, encoding)?,
            end: self.offset(end_line, end, encoding)?,
        };
        anyhow::ensure!(range.start <= range.end, "reversed SCIP range");
        Ok(range)
    }
}
struct LazyDocument {
    bytes: Vec<u8>,
    encoding: u64,
    source: Arc<Source>,
    calls: OnceLock<Vec<(ByteRange, String)>>,
}
impl LazyDocument {
    fn calls(&self) -> &[(ByteRange, String)] {
        self.calls.get_or_init(|| {
            // Wire and ranges were validated during load; malformed indexes never reach lookup.
            let Ok(doc) = Document::parse_from_bytes(&self.bytes) else {
                return vec![];
            };
            let mut calls: Vec<_> = doc
                .occurrences
                .into_iter()
                .filter(|o| o.symbol_roles & 1 == 0)
                .filter_map(|o| {
                    self.source
                        .range(&occurrence_range(&o).ok()?, self.encoding)
                        .ok()
                        .map(|range| (range, o.symbol))
                })
                .collect();
            calls.sort();
            calls.dedup();
            calls
        })
    }
}
#[derive(Debug, Clone, serde::Serialize)]
pub struct Documentation {
    pub signature: String,
    pub documentation: Vec<String>,
}
pub struct ScipResolver {
    documents: BTreeMap<RelPath, Vec<LazyDocument>>,
    definitions: BTreeMap<String, Def>,
    implementations: BTreeMap<String, Vec<String>>,
    documentation: BTreeMap<String, Documentation>,
}
impl ScipResolver {
    pub fn load(
        root: &Path,
        indexes: &[std::path::PathBuf],
        sources: &BTreeMap<String, Arc<str>>,
    ) -> Result<Self> {
        let metadata = crate::metadata::Metadata::load(root)?;
        Self::load_with_metadata(root, indexes, sources, metadata.as_ref())
    }
    pub fn load_with_metadata(
        root: &Path,
        indexes: &[std::path::PathBuf],
        sources: &BTreeMap<String, Arc<str>>,
        metadata: Option<&crate::metadata::Metadata>,
    ) -> Result<Self> {
        let mut result = Self {
            documents: BTreeMap::new(),
            definitions: BTreeMap::new(),
            implementations: BTreeMap::new(),
            documentation: BTreeMap::new(),
        };
        let mut indexes = indexes.to_vec();
        indexes.sort();
        let sources: BTreeMap<_, _> = sources
            .iter()
            .map(|(p, t)| (p.clone(), Arc::new(Source::new(t.clone()))))
            .collect();
        for index in indexes {
            let bytes = std::fs::read(&index)
                .with_context(|| format!("{}:1: read SCIP", index.display()))?;
            result
                .add_index(root, &index, &bytes, &sources, metadata)
                .with_context(|| format!("{}:1: invalid SCIP", index.display()))?;
        }
        for values in result.implementations.values_mut() {
            values.sort();
            values.dedup();
        }
        Ok(result)
    }
    fn symbol_info(&mut self, info: SymbolInformation) {
        for rel in &info.relationships {
            if rel.is_implementation {
                self.implementations
                    .entry(rel.symbol.clone())
                    .or_default()
                    .push(info.symbol.clone());
            }
        }
        let doc = Documentation {
            signature: info
                .signature_documentation
                .as_ref()
                .map(|d| d.text.clone())
                .unwrap_or_default(),
            documentation: info.documentation,
        };
        self.documentation.entry(info.symbol).or_insert(doc);
    }
    fn add_index(
        &mut self,
        root: &Path,
        index: &Path,
        bytes: &[u8],
        sources: &BTreeMap<String, Arc<Source>>,
        freshness: Option<&crate::metadata::Metadata>,
    ) -> Result<()> {
        let top = fields(bytes)?;
        let metadata = top
            .iter()
            .find(|f| f.number == 1)
            .map(|f| Metadata::parse_from_bytes(f.bytes))
            .transpose()?
            .unwrap_or_default();
        let ts = metadata
            .tool_info
            .as_ref()
            .is_some_and(|t| t.name.contains("typescript"));
        let index_root = index
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .unwrap_or(root);
        let prefix = index_root.strip_prefix(root).unwrap_or(Path::new(""));
        for field in top {
            if field.number == 3 {
                self.symbol_info(SymbolInformation::parse_from_bytes(field.bytes)?);
                continue;
            }
            if field.number != 2 {
                continue;
            }
            let document = fields(field.bytes)?;
            let path = std::str::from_utf8(
                document
                    .iter()
                    .find(|f| f.number == 1)
                    .context("SCIP document missing relative_path")?
                    .bytes,
            )?;
            anyhow::ensure!(
                !Path::new(path).is_absolute()
                    && !Path::new(path)
                        .components()
                        .any(|c| matches!(c, std::path::Component::ParentDir)),
                "unsafe SCIP document path {path}"
            );
            let path = prefix.join(path).to_string_lossy().replace('\\', "/");
            let Some(source) = sources.get(&path).cloned() else {
                continue;
            };
            let raw_encoding = document
                .iter()
                .find(|f| f.number == 6)
                .map(|f| f.int)
                .unwrap_or(0);
            let encoding = if raw_encoding == 0 {
                if ts {
                    2
                } else {
                    1
                }
            } else {
                raw_encoding
            };
            anyhow::ensure!(
                (1..=3).contains(&encoding),
                "unknown SCIP position encoding"
            );
            for field in &document {
                if field.number == 3 {
                    self.symbol_info(SymbolInformation::parse_from_bytes(field.bytes)?);
                } else if field.number == 2 {
                    let occurrence = Occurrence::parse_from_bytes(field.bytes)?;
                    let range =
                        if freshness.is_some_and(|m| !m.fresh(&path, source.text.as_bytes())) {
                            ByteRange { start: 0, end: 0 }
                        } else {
                            source
                                .range(&occurrence_range(&occurrence)?, encoding)
                                .with_context(|| format!("{path}: invalid occurrence"))?
                        };
                    if occurrence.symbol_roles & 1 != 0 && !occurrence.symbol.is_empty() {
                        let symbol = if occurrence.symbol.starts_with("local ") {
                            format!("{path}#{}", occurrence.symbol)
                        } else {
                            occurrence.symbol
                        };
                        self.definitions.entry(symbol.clone()).or_insert(Def {
                            symbol,
                            file: path.clone(),
                            range,
                        });
                    }
                }
            }
            self.documents.entry(path).or_default().push(LazyDocument {
                bytes: field.bytes.to_vec(),
                encoding,
                source,
                calls: OnceLock::new(),
            });
        }
        Ok(())
    }
    pub fn documentation(&self, symbol: &str) -> Option<&Documentation> {
        self.documentation.get(symbol)
    }
    pub fn definition(&self, symbol: &str) -> Option<&Def> {
        self.definitions.get(symbol)
    }
}
impl Resolver for ScipResolver {
    fn resolve(&self, file: &RelPath, callee: ByteRange) -> Resolution {
        let Some(documents) = self.documents.get(file) else {
            return Resolution::Unresolved;
        };
        for doc in documents {
            let calls = doc.calls();
            let first = calls.partition_point(|(range, _)| *range < callee);
            for (range, raw_symbol) in calls[first..]
                .iter()
                .take_while(|(range, _)| *range == callee)
            {
                let _ = range;
                let symbol = if raw_symbol.starts_with("local ") {
                    format!("{file}#{raw_symbol}")
                } else {
                    raw_symbol.clone()
                };
                let impls: Vec<_> = self
                    .implementations(&symbol)
                    .iter()
                    .filter_map(|s| self.definitions.get(s).cloned())
                    .collect();
                if !impls.is_empty() {
                    return Resolution::Impls { symbol, impls };
                }
                if let Some(def) = self.definitions.get(&symbol) {
                    return def.clone().into();
                }
                if let Ok(parsed) = scip::symbol::parse_symbol(&symbol) {
                    if let Some(package) = parsed.package.as_ref() {
                        return Resolution::External {
                            symbol,
                            package: package.name.clone(),
                        };
                    }
                }
            }
        }
        Resolution::Unresolved
    }
    fn implementations(&self, symbol: &str) -> Vec<String> {
        self.implementations
            .get(symbol)
            .cloned()
            .unwrap_or_default()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_position_encodings() {
        let source = Source::new(Arc::from("😀é score\nnext"));
        for (encoding, start, end) in [(1, 7, 12), (2, 4, 9), (3, 3, 8)] {
            assert_eq!(
                source.range(&[0, start, end], encoding).unwrap(),
                ByteRange { start: 7, end: 12 }
            );
        }
        assert!(source.range(&[0, 1, 2], 2).is_err());
    }
    #[test]
    fn checked_wire_rejects_truncation() {
        assert!(fields(&[18, 20, 0]).is_err());
        assert!(fields(&[0]).is_err());
    }
}
