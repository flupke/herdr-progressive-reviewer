//! The marks an agent may add to its Markdown beyond standard Markdown, each written as a
//! `[!NAME]` marker: a quote that opens with a [`Callout`] marker is a callout, and a table cell
//! that opens with a [`StatusMark`] marker carries that status. The page and the pane both read
//! them, and the Explore prompt names them, from these types. Markers are read without regard
//! to case.

#[cfg(test)]
mod tests;

/// A kind of mark, with its marker.
pub trait Mark: Copy + Sized + 'static {
    /// Every mark of the kind.
    const ALL: &'static [Self];

    fn spelling(self) -> Spelling;

    /// The marker that opens the text the mark applies to.
    fn marker(self) -> &'static str {
        self.spelling().marker
    }

    /// The mark's name as the reader sees it.
    fn label(self) -> &'static str {
        self.spelling().label
    }

    /// The mark's name in lower case, for a class name.
    fn name(self) -> &'static str {
        self.spelling().name
    }

    /// The mark whose marker opens `text`, after any leading space, and the text after the
    /// marker and the space that follows it.
    fn strip(text: &str) -> Option<(Self, &str)> {
        let text = text.trim_start();
        Self::ALL.iter().find_map(|&mark| {
            let marker = mark.marker();
            let opening = text.get(..marker.len())?;
            opening
                .eq_ignore_ascii_case(marker)
                .then(|| (mark, text[marker.len()..].trim_start()))
        })
    }
}

/// How a mark is written and shown.
#[derive(Clone, Copy, Debug)]
pub struct Spelling {
    marker: &'static str,
    label: &'static str,
    name: &'static str,
}

/// The kind of a callout: a quote that opens with its marker, as in `> [!TIP] Run it twice.`
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Callout {
    Conclusion,
    Tip,
    Warning,
    Error,
}

impl Mark for Callout {
    const ALL: &'static [Self] = &[Self::Conclusion, Self::Tip, Self::Warning, Self::Error];

    fn spelling(self) -> Spelling {
        let (marker, label, name) = match self {
            Self::Conclusion => ("[!CONCLUSION]", "Conclusion", "conclusion"),
            Self::Tip => ("[!TIP]", "Tip", "tip"),
            Self::Warning => ("[!WARNING]", "Warning", "warning"),
            Self::Error => ("[!ERROR]", "Error", "error"),
        };
        Spelling {
            marker,
            label,
            name,
        }
    }
}

/// The status of a table cell: a cell that opens with its marker, as in `| [!good] 2 ms |`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusMark {
    Good,
    Bad,
    Warning,
}

impl StatusMark {
    /// The mark as the reader sees it in place of the marker.
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Good => "✓",
            Self::Bad => "✗",
            Self::Warning => "!",
        }
    }
}

impl Mark for StatusMark {
    const ALL: &'static [Self] = &[Self::Good, Self::Bad, Self::Warning];

    fn spelling(self) -> Spelling {
        let (marker, label, name) = match self {
            Self::Good => ("[!good]", "Good", "good"),
            Self::Bad => ("[!bad]", "Bad", "bad"),
            Self::Warning => ("[!warning]", "Warning", "warning"),
        };
        Spelling {
            marker,
            label,
            name,
        }
    }
}
