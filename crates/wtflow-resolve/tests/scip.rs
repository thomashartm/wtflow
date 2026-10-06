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
