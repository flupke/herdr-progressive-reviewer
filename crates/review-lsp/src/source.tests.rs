use std::fs;

use std::str::FromStr;

use lsp_types::{
    GotoDefinitionResponse, HoverContents, LanguageString, Location, LocationLink, MarkedString,
    MarkupContent, MarkupKind, Position, PositionEncodingKind, Range, Uri,
};

use super::{ServerLocation, decoded_column, encoded_column, hover_markdown, path_uri, uri_path};

#[test]
fn file_uris_round_trip_spaces_and_unicode() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("a b-λ.rs");
    fs::write(&path, "fn main() {}\n").unwrap();
    let uri = path_uri(&path).unwrap();

    assert_eq!(uri_path(&uri).unwrap(), path.canonicalize().unwrap());
}

#[test]
fn columns_use_the_negotiated_encoding() {
    let line = "aλ😀z";
    let byte = "aλ😀".len();

    assert_eq!(
        encoded_column(line, byte, &PositionEncodingKind::UTF8).unwrap(),
        7
    );
    assert_eq!(
        encoded_column(line, byte, &PositionEncodingKind::UTF16).unwrap(),
        4
    );
    assert_eq!(
        encoded_column(line, byte, &PositionEncodingKind::UTF32).unwrap(),
        3
    );
    assert_eq!(
        decoded_column(line, 7, &PositionEncodingKind::UTF8),
        Some(byte)
    );
    assert_eq!(
        decoded_column(line, 4, &PositionEncodingKind::UTF16),
        Some(byte)
    );
    assert_eq!(
        decoded_column(line, 3, &PositionEncodingKind::UTF32),
        Some(byte)
    );
}

#[test]
fn columns_reject_invalid_boundaries_and_partial_encoding_units() {
    let line = "aλ😀z";
    assert!(encoded_column(line, 2, &PositionEncodingKind::UTF8).is_err());
    assert_eq!(decoded_column(line, 2, &PositionEncodingKind::UTF8), None);
    assert_eq!(decoded_column(line, 3, &PositionEncodingKind::UTF16), None);
    assert_eq!(decoded_column(line, 99, &PositionEncodingKind::UTF32), None);
    assert_eq!(
        decoded_column(line, 5, &PositionEncodingKind::UTF16),
        Some(line.len())
    );
}

#[test]
fn definition_response_shapes_preserve_the_selected_ranges() {
    let uri = Uri::from_str("file:///tmp/source.rs").unwrap();
    let first = Location::new(
        uri.clone(),
        Range::new(Position::new(2, 3), Position::new(4, 5)),
    );
    let scalar =
        ServerLocation::from_definition(Some(GotoDefinitionResponse::Scalar(first.clone())));
    assert_eq!(scalar.len(), 1);
    assert_eq!(scalar[0].path, std::path::PathBuf::from("/tmp/source.rs"));
    assert_eq!((scalar[0].line, scalar[0].character), (2, 3));
    assert_eq!((scalar[0].end_line, scalar[0].end_character), (4, 5));

    let array = ServerLocation::from_definition(Some(GotoDefinitionResponse::Array(vec![first])));
    assert_eq!(array.len(), 1);
    let link = LocationLink {
        origin_selection_range: None,
        target_uri: uri,
        target_range: Range::new(Position::new(1, 1), Position::new(9, 9)),
        target_selection_range: Range::new(Position::new(6, 7), Position::new(8, 9)),
    };
    let linked = ServerLocation::from_definition(Some(GotoDefinitionResponse::Link(vec![link])));
    assert_eq!((linked[0].line, linked[0].character), (6, 7));
    assert_eq!((linked[0].end_line, linked[0].end_character), (8, 9));
    assert!(ServerLocation::from_definition(None).is_empty());
}

#[test]
fn server_locations_normalize_encoded_columns_from_disk() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("source.rs");
    fs::write(&path, "aλ😀z\nsecond\n").unwrap();
    let normalized = ServerLocation {
        path: path.clone(),
        line: 0,
        character: 4,
        end_line: 1,
        end_character: 3,
    }
    .normalize(&PositionEncodingKind::UTF16)
    .unwrap();
    assert_eq!(normalized.path, path);
    assert_eq!(normalized.byte_column, "aλ😀".len());
    assert_eq!(normalized.end_byte_column, 3);

    assert!(
        ServerLocation {
            path: directory.path().join("missing.rs"),
            line: 0,
            character: 0,
            end_line: 0,
            end_character: 0,
        }
        .normalize(&PositionEncodingKind::UTF8)
        .is_none()
    );
}

#[test]
fn hover_content_keeps_markup_and_formats_code() {
    assert_eq!(
        hover_markdown(HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: "documentation".to_owned(),
        })),
        "documentation"
    );
    assert_eq!(
        hover_markdown(HoverContents::Scalar(MarkedString::LanguageString(
            LanguageString {
                language: "rust".to_owned(),
                value: "fn main() {}".to_owned(),
            }
        ))),
        "```rust\nfn main() {}\n```"
    );
    assert_eq!(
        hover_markdown(HoverContents::Array(vec![
            MarkedString::String("first".to_owned()),
            MarkedString::String("second".to_owned()),
        ])),
        "first\n\nsecond"
    );
}
