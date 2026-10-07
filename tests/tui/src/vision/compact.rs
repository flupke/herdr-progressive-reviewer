//! The screen's text as the tools return it: each row numbered, without the spaces and box
//! borders that end it, and the rows that show nothing folded into one line that names them, so
//! an agent reads what the reviewer painted and not the blanks around it. A box still shows by
//! its corners, and the rows keep their columns, so a cell still counts from the start of its
//! row, after its `row: ` number.

use unicode_width::UnicodeWidthChar;

/// The characters that draw the vertical side of a box.
const SIDES: &[char] = &['│', '┃', '║', '╎', '╏', '┆', '┇', '┊', '┋'];

/// The characters that end or join a box's side, above or below it.
const JOINS: &[char] = &[
    '╭', '╮', '╰', '╯', '┌', '┐', '└', '┘', '┏', '┓', '┗', '┛', '╔', '╗', '╚', '╝', '├', '┤', '┬',
    '┴', '┼', '┣', '┫', '┳', '┻', '╋', '╠', '╣', '╦', '╩', '╬',
];

/// The half and full blocks a QR code is drawn with, two modules per cell.
const QR_BLOCKS: &[char] = &['▀', '▄', '█'];

/// The smallest QR code, 21 modules square, fills 21 columns and 11 rows.
const QR_MIN_COLUMNS: usize = 21;
const QR_MIN_ROWS: usize = 11;

/// One line of the returned text, standing for the zero-based rows `first..=last`.
struct Line {
    first: usize,
    last: usize,
    kind: Kind,
    text: String,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Kind {
    /// One row, without the spaces and borders that end it.
    Row,
    /// A run of rows that show nothing.
    Empty,
    /// The rows of a QR code.
    QrCode,
}

impl Line {
    /// The line as returned: a row after its number, a fold as it is, since it names its rows.
    fn shown(self) -> String {
        match self.kind {
            Kind::Row if self.text.is_empty() => format!("{}:", self.first),
            Kind::Row => format!("{}: {}", self.first, self.text),
            Kind::Empty | Kind::QrCode => self.text,
        }
    }
}

/// The rows that changed between two screens of the same size.
pub(crate) struct Changes {
    /// How many rows show another text, once trimmed.
    pub(crate) count: usize,
    /// The lines of the new screen that hold a changed row.
    pub(crate) text: String,
}

/// The whole screen `text`, one line per row, folded.
pub(crate) fn whole(text: &str) -> String {
    lines(&trimmed(text))
        .into_iter()
        .map(Line::shown)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The lines of `after` that hold a row whose text, once trimmed, differs from the same row of
/// `previous`.
pub(crate) fn changes(previous: &str, after: &str) -> Changes {
    let previous = trimmed(previous);
    let after = trimmed(after);
    let changed = |row: usize| previous.get(row) != after.get(row);
    let text = lines(&after)
        .into_iter()
        .filter(|line| (line.first..=line.last).any(changed))
        .map(Line::shown)
        .collect::<Vec<_>>()
        .join("\n");
    Changes {
        count: (0..previous.len().max(after.len()))
            .filter(|&row| changed(row))
            .count(),
        text,
    }
}

/// The rows of `text` without the spaces and box borders that end them. A side is a border
/// when a side or a corner lines up with it in the row above or below: a lone `│` is text.
fn trimmed(text: &str) -> Vec<String> {
    let rows: Vec<Vec<(usize, char)>> = text.lines().map(cells).collect();
    let joins = |index: Option<usize>, column: usize| {
        index.and_then(|index| rows.get(index)).is_some_and(|row| {
            row.iter().any(|&(at, character)| {
                at == column && (SIDES.contains(&character) || JOINS.contains(&character))
            })
        })
    };
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            let border =
                |column| joins(index.checked_sub(1), column) || joins(Some(index + 1), column);
            let end = row
                .iter()
                .rposition(|&(column, character)| {
                    character != ' ' && !(SIDES.contains(&character) && border(column))
                })
                .map_or(0, |last| last + 1);
            row[..end].iter().map(|&(_, character)| character).collect()
        })
        .collect()
}

/// The characters of `row`, each with the column it starts at.
fn cells(row: &str) -> Vec<(usize, char)> {
    let mut column = 0;
    row.chars()
        .map(|character| {
            let cell = (column, character);
            column += character.width().unwrap_or(0);
            cell
        })
        .collect()
}

fn lines(rows: &[String]) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut first = 0;
    while first < rows.len() {
        let row = &rows[first];
        if let Some(code) = QrRow::of(row) {
            let height = rows[first..]
                .iter()
                .take_while(|row| QrRow::of(row).is_some_and(|other| other.aligned(&code)))
                .count();
            if height >= QR_MIN_ROWS {
                let last = first + height - 1;
                lines.push(Line {
                    first,
                    last,
                    kind: Kind::QrCode,
                    text: format!("{}[QR code, rows {first}-{last}]", code.indent),
                });
                first = last + 1;
                continue;
            }
        }
        let height = rows[first..]
            .iter()
            .take_while(|row| row.is_empty())
            .count();
        if height > 1 {
            let last = first + height - 1;
            lines.push(Line {
                first,
                last,
                kind: Kind::Empty,
                text: format!("[empty rows {first}-{last}]"),
            });
            first = last + 1;
            continue;
        }
        lines.push(Line {
            first,
            last: first,
            kind: Kind::Row,
            text: row.clone(),
        });
        first += 1;
    }
    lines
}

/// A trimmed row that shows only a stretch of QR blocks after box sides and spaces.
struct QrRow<'a> {
    /// The sides and spaces before the blocks.
    indent: &'a str,
    /// The columns from the first block to the last.
    columns: usize,
}

impl<'a> QrRow<'a> {
    fn of(row: &'a str) -> Option<Self> {
        let start = row.find(|character| character != ' ' && !SIDES.contains(&character))?;
        let mut columns = 0;
        for character in row[start..].chars() {
            if character != ' ' && !QR_BLOCKS.contains(&character) {
                return None;
            }
            columns += 1;
        }
        (columns >= QR_MIN_COLUMNS).then_some(Self {
            indent: &row[..start],
            columns,
        })
    }

    /// Whether this row continues the code that `other` belongs to: the same columns.
    fn aligned(&self, other: &Self) -> bool {
        self.indent.chars().count() == other.indent.chars().count() && self.columns == other.columns
    }
}

#[cfg(test)]
#[path = "compact.tests.rs"]
mod tests;
