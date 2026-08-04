use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use lsp_types::{
    GotoDefinitionResponse, HoverContents, Location, MarkedString, PositionEncodingKind, Uri,
};
use url::Url;

use crate::api::SourceLocation;

pub(super) struct ServerLocation {
    pub(super) path: PathBuf,
    pub(super) line: u32,
    pub(super) character: u32,
    pub(super) end_line: u32,
    pub(super) end_character: u32,
}

impl ServerLocation {
    pub(super) fn from_definition(response: Option<GotoDefinitionResponse>) -> Vec<Self> {
        match response {
            Some(GotoDefinitionResponse::Scalar(location)) => {
                Self::from_lsp(&location).into_iter().collect()
            }
            Some(GotoDefinitionResponse::Array(locations)) => {
                locations.iter().filter_map(Self::from_lsp).collect()
            }
            Some(GotoDefinitionResponse::Link(locations)) => locations
                .into_iter()
                .filter_map(|link| Self::from_range(&link.target_uri, link.target_selection_range))
                .collect(),
            None => Vec::new(),
        }
    }

    pub(super) fn from_lsp(location: &Location) -> Option<Self> {
        Self::from_range(&location.uri, location.range)
    }

    fn from_range(uri: &Uri, range: lsp_types::Range) -> Option<Self> {
        Some(Self {
            path: uri_path(uri)?,
            line: range.start.line,
            character: range.start.character,
            end_line: range.end.line,
            end_character: range.end.character,
        })
    }

    pub(super) fn normalize(self, encoding: &PositionEncodingKind) -> Option<SourceLocation> {
        let text = fs::read_to_string(&self.path).ok()?;
        let start = text.lines().nth(usize::try_from(self.line).ok()?)?;
        let end = text.lines().nth(usize::try_from(self.end_line).ok()?)?;
        Some(SourceLocation {
            path: self.path,
            line: self.line,
            byte_column: decoded_column(start, self.character, encoding)?,
            end_line: self.end_line,
            end_byte_column: decoded_column(end, self.end_character, encoding)?,
        })
    }
}

pub(super) fn encoded_column(
    line: &str,
    byte: usize,
    encoding: &PositionEncodingKind,
) -> Result<u32, String> {
    let prefix = line
        .get(..byte)
        .ok_or_else(|| "the source column is invalid".to_owned())?;
    let count = if *encoding == PositionEncodingKind::UTF8 {
        prefix.len()
    } else if *encoding == PositionEncodingKind::UTF32 {
        prefix.chars().count()
    } else {
        prefix.encode_utf16().count()
    };
    u32::try_from(count).map_err(|_| "the source column is too large".to_owned())
}

pub(super) fn decoded_column(
    line: &str,
    units: u32,
    encoding: &PositionEncodingKind,
) -> Option<usize> {
    if *encoding == PositionEncodingKind::UTF8 {
        let byte = usize::try_from(units).ok()?;
        return line.is_char_boundary(byte).then_some(byte);
    }
    let mut count = 0_u32;
    for (byte, character) in line.char_indices() {
        if count == units {
            return Some(byte);
        }
        count = count.saturating_add(if *encoding == PositionEncodingKind::UTF32 {
            1
        } else {
            u32::try_from(character.len_utf16()).ok()?
        });
        if count > units {
            return None;
        }
    }
    (count == units).then_some(line.len())
}

pub(super) fn hover_markdown(contents: HoverContents) -> String {
    match contents {
        HoverContents::Markup(content) => content.value,
        HoverContents::Scalar(marked) => marked_string(marked),
        HoverContents::Array(marked) => marked
            .into_iter()
            .map(marked_string)
            .collect::<Vec<_>>()
            .join("\n\n"),
    }
}

fn marked_string(marked: MarkedString) -> String {
    match marked {
        MarkedString::String(text) => text,
        MarkedString::LanguageString(code) => {
            format!("```{}\n{}\n```", code.language, code.value)
        }
    }
}

pub(super) fn path_uri(path: &Path) -> Result<Uri, String> {
    let path = path
        .canonicalize()
        .map_err(|error| format!("could not resolve {}: {error}", path.display()))?;
    let url = Url::from_file_path(path).map_err(|()| "could not create a file URI".to_owned())?;
    Uri::from_str(url.as_str()).map_err(|error| error.to_string())
}

fn uri_path(uri: &Uri) -> Option<PathBuf> {
    Url::parse(uri.as_str()).ok()?.to_file_path().ok()
}

#[cfg(test)]
mod tests {
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
}
