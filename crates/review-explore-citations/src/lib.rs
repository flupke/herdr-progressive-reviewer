//! A question's citations with their cited lines in syntax colors, as the Explore page shows
//! them. The colors are Catppuccin Mocha's, named by their role in the Catppuccin palette, so
//! that a page can show each role in the palette's light flavor too.

use ratatui::style::Color;
use review_explore::{CitedLines, EvidenceRef, Uncitable};
use syntax_highlighting::SyntaxHighlighter;
pub use syntax_highlighting::{HighlightedRow, Token};
use two_face::theme::EmbeddedThemeName;

/// A citation of a question, and the lines it shows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Citation {
    pub evidence: EvidenceRef,
    /// The cited lines as colored diff rows, or why the citation shows none.
    pub lines: Result<Vec<HighlightedRow>, Uncitable>,
}

/// Colors cited code with Catppuccin Mocha.
#[derive(Clone, Debug)]
pub struct CodeColors(SyntaxHighlighter);

/// The Catppuccin roles that the Mocha syntax theme uses, with their Mocha color. A page's
/// stylesheet gives each role its color.
const PALETTE: &[(&str, (u8, u8, u8))] = &[
    ("rosewater", (0xf5, 0xe0, 0xdc)),
    ("flamingo", (0xf2, 0xcd, 0xcd)),
    ("pink", (0xf5, 0xc2, 0xe7)),
    ("mauve", (0xcb, 0xa6, 0xf7)),
    ("red", (0xf3, 0x8b, 0xa8)),
    ("maroon", (0xeb, 0xa0, 0xac)),
    ("peach", (0xfa, 0xb3, 0x87)),
    ("yellow", (0xf9, 0xe2, 0xaf)),
    ("green", (0xa6, 0xe3, 0xa1)),
    ("teal", (0x94, 0xe2, 0xd5)),
    ("sky", (0x89, 0xdc, 0xeb)),
    ("sapphire", (0x74, 0xc7, 0xec)),
    ("blue", (0x89, 0xb4, 0xfa)),
    ("lavender", (0xb4, 0xbe, 0xfe)),
    ("overlay2", (0x93, 0x99, 0xb2)),
];

impl Default for CodeColors {
    fn default() -> Self {
        // Plain text, and Mocha's own text color, have no role: they take the page's text
        // color.
        Self(SyntaxHighlighter::new(
            EmbeddedThemeName::CatppuccinMocha,
            Color::Reset,
        ))
    }
}

impl CodeColors {
    /// The citation `evidence`, showing `lines`, the lines that the change has for it.
    pub fn cite(&self, evidence: EvidenceRef, lines: Result<CitedLines, Uncitable>) -> Citation {
        let lines = lines.map(|lines| {
            self.0
                .highlight(
                    &lines.path,
                    lines.rows,
                    lines.old_content.as_deref(),
                    lines.new_content.as_deref(),
                )
                .rows
        });
        Citation { evidence, lines }
    }

    /// The palette role of `token`'s color, or `None` for the text color.
    pub fn role(token: &Token) -> Option<&'static str> {
        let Color::Rgb(red, green, blue) = token.color else {
            return None;
        };
        PALETTE
            .iter()
            .find(|(_, color)| *color == (red, green, blue))
            .map(|(role, _)| *role)
    }
}

#[cfg(test)]
mod tests {
    use review_explore::{CitedLines, CodeLocation, EvidenceRef, SourceSide, Uncitable};
    use review_repository::diff::DiffRow;
    use review_repository::repository::RepoPath;

    use super::CodeColors;

    fn evidence() -> EvidenceRef {
        EvidenceRef {
            location: CodeLocation {
                path: RepoPath::from_bytes("main.rs"),
                side: SourceSide::New,
                lines: None,
            },
            notes: "The entry point.".into(),
        }
    }

    #[test]
    fn cited_rows_get_the_roles_of_their_syntax() {
        let lines = CitedLines {
            path: "main.rs".into(),
            rows: vec![DiffRow::Add {
                new_line: 1,
                text: "+fn main() {}".into(),
            }],
            old_content: None,
            new_content: Some(b"fn main() {}\n".to_vec()),
        };

        let citation = CodeColors::default().cite(evidence(), Ok(lines));

        let rows = citation.lines.unwrap();
        let tokens: Vec<_> = rows[0]
            .tokens
            .iter()
            .map(|token| (token.text.as_str(), CodeColors::role(token)))
            .collect();
        assert_eq!(tokens[0], ("fn", Some("mauve")));
        assert_eq!(tokens[2], ("main", Some("blue")));
        assert_eq!(
            rows[0].diff,
            DiffRow::Add {
                new_line: 1,
                text: "+fn main() {}".into()
            }
        );
        assert_eq!(citation.evidence, evidence());
    }

    #[test]
    fn a_citation_without_lines_keeps_why() {
        let citation = CodeColors::default().cite(evidence(), Err(Uncitable::WholeFile));
        assert_eq!(citation.lines, Err(Uncitable::WholeFile));
    }
}
