use protobuf::{EnumOrUnknown, Message, MessageField};
use scip::types::{Document, Index, Metadata, Occurrence, PositionEncoding, ToolInfo};
use std::{collections::BTreeMap, sync::Arc};
use wtflow_resolve::{scip::ScipResolver, ByteRange, Resolution, Resolver};
#[test]
fn encoding_defaults_exact_lookup_and_definition_role() {
    for (tool, encoding, start, end) in [
        ("scip-typescript", 0, 4, 9),
        ("scip-python", 0, 7, 12),
        ("any", 1, 7, 12),
        ("any", 2, 4, 9),
        ("any", 3, 3, 8),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let index_path = dir.path().join(".wtflow/index/test.scip");
        std::fs::create_dir_all(index_path.parent().unwrap()).unwrap();
        let symbol = "scip-typescript npm fixture 1.0.0 score().";
        let mut doc = Document {
            relative_path: "x.ts".into(),
            position_encoding: EnumOrUnknown::from_i32(encoding),
            ..Document::default()
        };
        doc.occurrences = vec![
            Occurrence {
                range: vec![0, start, end],
                symbol: symbol.into(),
                symbol_roles: 1,
                ..Occurrence::default()
            },
            Occurrence {
                range: vec![1, start, end],
                symbol: symbol.into(),
                ..Occurrence::default()
            },
        ];
        let index = Index {
            metadata: MessageField::some(Metadata {
                tool_info: MessageField::some(ToolInfo {
                    name: tool.into(),
                    ..ToolInfo::default()
                }),
                ..Metadata::default()
            }),
            documents: vec![doc],
            ..Index::default()
        };
        std::fs::write(&index_path, index.write_to_bytes().unwrap()).unwrap();
        let sources = BTreeMap::from([("x.ts".into(), Arc::<str>::from("😀é score\n😀é score"))]);
        let resolver = ScipResolver::load(dir.path(), &[index_path], &sources).unwrap();
        let path = "x.ts".to_owned();
        assert!(
            matches!(
                resolver.resolve(&path, ByteRange { start: 20, end: 25 }),
                Resolution::Def {
                    range: ByteRange { start: 7, end: 12 },
                    ..
                }
            ),
            "{tool}/{encoding}"
        );
        assert_eq!(
            resolver.resolve(&path, ByteRange { start: 7, end: 12 }),
            Resolution::Unresolved
        );
        assert_eq!(
            resolver.resolve(&path, ByteRange { start: 20, end: 24 }),
            Resolution::Unresolved
        );
    }
    assert_eq!(PositionEncoding::UTF16CodeUnitOffsetFromLineStart as i32, 2);
}

#[test]
fn typescript_unicode_line_separators_and_crlf_map_to_original_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".wtflow/index/typescript.scip");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let source = "'Anna\u{2028}Keller';\r\n'Anna\u{2029}Keller';\rscore();";
    let definition = "scip-typescript npm example 1.0.0 score().";
    let index = Index {
        metadata: MessageField::some(Metadata {
            tool_info: MessageField::some(ToolInfo {
                name: "scip-typescript".into(),
                ..ToolInfo::default()
            }),
            ..Metadata::default()
        }),
        documents: vec![Document {
            relative_path: "x.ts".into(),
            occurrences: vec![Occurrence {
                range: vec![4, 0, 5],
                symbol: definition.into(),
                ..Occurrence::default()
            }],
            ..Document::default()
        }],
        ..Index::default()
    };
    std::fs::write(&path, index.write_to_bytes().unwrap()).unwrap();
    let sources = BTreeMap::from([("x.ts".into(), Arc::<str>::from(source))]);
    let resolver = ScipResolver::load(dir.path(), &[path], &sources).unwrap();
    let start = source.find("score()").unwrap();
    assert!(matches!(
        resolver.resolve(
            &"x.ts".into(),
            ByteRange {
                start,
                end: start + 5
            }
        ),
        Resolution::External { .. }
    ));
}

#[test]
fn malformed_documentation_does_not_disable_call_resolution_but_symbols_stay_strict() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".wtflow/index/typescript.scip");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let symbol = "scip-typescript npm example 1.0.0 score().";
    let index = Index {
        documents: vec![Document {
            relative_path: "x.ts".into(),
            occurrences: vec![Occurrence {
                range: vec![0, 0, 5],
                symbol: symbol.into(),
                ..Occurrence::default()
            }],
            symbols: vec![scip::types::SymbolInformation {
                symbol: symbol.into(),
                documentation: vec!["docSURrogate".into()],
                ..scip::types::SymbolInformation::default()
            }],
            ..Document::default()
        }],
        ..Index::default()
    };
    let mut bytes = index.write_to_bytes().unwrap();
    let offset = bytes.windows(3).position(|s| s == b"SUR").unwrap();
    bytes[offset..offset + 3].copy_from_slice(&[0xed, 0xa0, 0x80]);
    std::fs::write(&path, &bytes).unwrap();
    let sources = BTreeMap::from([("x.ts".into(), Arc::<str>::from("score()"))]);
    let resolver = ScipResolver::load(dir.path(), &[path.clone()], &sources).unwrap();
    assert!(resolver.documentation(symbol).unwrap().documentation[0].contains('\u{fffd}'));
    assert!(matches!(
        resolver.resolve(&"x.ts".into(), ByteRange { start: 0, end: 5 }),
        Resolution::External { .. }
    ));
    let offset = bytes.windows(5).position(|s| s == b"score").unwrap();
    bytes[offset..offset + 3].copy_from_slice(&[0xed, 0xa0, 0x80]);
    std::fs::write(&path, bytes).unwrap();
    assert!(ScipResolver::load(dir.path(), &[path], &sources).is_err());
}
