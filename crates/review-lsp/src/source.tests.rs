use std::fs;

use lsp_types::PositionEncodingKind;

use super::{decoded_column, encoded_column, path_uri, uri_path};

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
