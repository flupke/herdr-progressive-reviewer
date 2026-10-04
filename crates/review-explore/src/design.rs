//! The design of the change, which the first turn of a round explains before its first question.

use std::borrow::Cow;
use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use serde::{Deserialize, Deserializer, Serialize};

use crate::Exploration;

/// What a reviewer needs to explain the change at a whiteboard without having written it: one
/// sentence for the whole change, then four parts, each shown under its own heading in the pane
/// and on the Explore page.
///
/// A design saved before theses existed has none: [`Design::thesis`] and [`Design::parts`] give a
/// stand-in, so whatever shows a design always has a thesis to show.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Design {
    /// The change in one sentence, the one you would say first at a whiteboard: what it does and
    /// how, naming the parts it builds on.
    #[schemars(required)]
    #[serde(skip_serializing_if = "Option::is_none")]
    thesis: Option<String>,
    /// What the change adds and where: the components it creates or changes, and how they fit
    /// into the code around them.
    overview: DesignPart,
    /// The main types, the data they hold and store, and how data flows through them.
    data_flow: DesignPart,
    /// The algorithm and its cost: time, memory, storage, I/O or calls to other systems.
    algorithm: DesignPart,
    /// The alternatives the implementer rejected, and why, as the description or the code
    /// states them; an inferred alternative says it is inferred.
    alternatives: DesignPart,
}

/// One part of the design: its thesis, then its Markdown. Serde is written by hand below;
/// `deny_unknown_fields` here only tells the tool's schema what its deserializer refuses.
#[derive(Clone, Debug, Eq, PartialEq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DesignPart {
    /// The part in one sentence, the one you would say at a whiteboard, naming the things it
    /// talks about.
    #[schemars(required)]
    thesis: Option<String>,
    /// The part's explanation in Markdown, which the thesis sums up and does not repeat.
    body: String,
}

/// A part of the design as the reviewer reads it: under its heading, its thesis, then the rest
/// of its Markdown.
#[derive(Clone, Debug, Serialize, Eq, PartialEq)]
pub struct DesignSection<'a> {
    pub title: &'static str,
    pub thesis: Cow<'a, str>,
    pub body: Cow<'a, str>,
}

impl Design {
    pub fn new(
        thesis: impl Into<String>,
        overview: DesignPart,
        data_flow: DesignPart,
        algorithm: DesignPart,
        alternatives: DesignPart,
    ) -> Self {
        Self {
            thesis: Some(thesis.into()),
            overview,
            data_flow,
            algorithm,
            alternatives,
        }
    }

    /// The change in one sentence. A design saved before theses existed has none: the first
    /// paragraph of "What it adds and where" stands in, the part closest to a statement of the
    /// whole change.
    pub fn thesis(&self) -> Cow<'_, str> {
        match &self.thesis {
            Some(thesis) => Cow::Borrowed(thesis),
            None => self.overview.text().thesis,
        }
    }

    /// The parts, in reading order, under their headings, each with its thesis, or the stand-in
    /// for a design saved before theses existed. A page message carries these and
    /// [`Design::thesis`], never the design itself, which saves such a design as it was.
    pub fn parts(&self) -> [DesignSection<'_>; 4] {
        [
            ("What it adds and where", self.overview_text()),
            ("Types and data flow", self.data_flow.text()),
            ("Algorithm and cost", self.algorithm.text()),
            ("Rejected alternatives", self.alternatives.text()),
        ]
        .map(|(title, text)| DesignSection {
            title,
            thesis: text.thesis,
            body: text.body,
        })
    }

    /// The overview's text. In a design saved before theses existed, its first paragraph stands
    /// in for the change's thesis, so the next one stands in for the overview's. Without a next
    /// one, the overview shares the change's thesis.
    fn overview_text(&self) -> PartText<'_> {
        let text = self.overview.text();
        if self.thesis.is_some() || Lead::of(&text.body).paragraph.is_none() {
            return text;
        }
        let rest = PartText::of(&text.body);
        PartText {
            thesis: Cow::Owned(rest.thesis.into_owned()),
            body: Cow::Owned(rest.body.into_owned()),
        }
    }

    /// About how many minutes the design takes to read: the words of its theses and parts at
    /// 230 a minute, rounded to the nearest minute, and never less than one. A diagram's source,
    /// or any other block of code, is not read as text and does not count.
    pub fn reading_minutes(&self) -> usize {
        const WORDS_A_MINUTE: usize = 230;
        let thesis = self.thesis();
        let parts = self.parts();
        let texts = parts.iter().flat_map(|part| [&part.thesis, &part.body]);
        let words: usize = std::iter::once(&thesis)
            .chain(texts)
            .map(|text| words_read(text.as_ref()))
            .sum();
        ((words + WORDS_A_MINUTE / 2) / WORDS_A_MINUTE).max(1)
    }

    /// The agent's design: a thesis for the change, and a thesis and text for every part.
    pub(crate) fn validate(&self) -> eyre::Result<()> {
        eyre::ensure!(
            self.thesis.as_deref().is_some_and(Self::is_one_line),
            "The design needs a thesis: the change in one sentence, on one line"
        );
        let parts = [
            &self.overview,
            &self.data_flow,
            &self.algorithm,
            &self.alternatives,
        ];
        eyre::ensure!(
            parts
                .iter()
                .all(|part| part.thesis.as_deref().is_some_and(Self::is_one_line)),
            "Every part of design needs a thesis: the part in one sentence, on one line"
        );
        eyre::ensure!(
            parts.iter().all(|part| !part.body.trim().is_empty()),
            "Every part of design needs text; say so when a part does not apply"
        );
        Ok(())
    }

    /// A thesis is one sentence; the tool can tell that it is there and on one line.
    fn is_one_line(thesis: &str) -> bool {
        !thesis.trim().is_empty() && !thesis.trim().contains('\n')
    }
}

impl DesignPart {
    pub fn new(thesis: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            thesis: Some(thesis.into()),
            body: body.into(),
        }
    }

    /// The part's thesis and the Markdown after it, or, for a part saved before theses existed,
    /// the text that stands in for its thesis and the rest.
    fn text(&self) -> PartText<'_> {
        match &self.thesis {
            Some(thesis) => PartText {
                thesis: Cow::Borrowed(thesis),
                body: Cow::Borrowed(&self.body),
            },
            None => PartText::of(&self.body),
        }
    }
}

/// The words a reader reads in `markdown`: its text and inline code, not its blocks of code.
fn words_read(markdown: &str) -> usize {
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    let mut in_code = false;
    let mut words = 0;
    for event in Parser::new_ext(markdown, options) {
        match event {
            Event::Start(Tag::CodeBlock(_)) => in_code = true,
            Event::End(TagEnd::CodeBlock) => in_code = false,
            Event::Text(text) | Event::Code(text) if !in_code => {
                words += text.split_whitespace().count();
            }
            _ => {}
        }
    }
    words
}

/// A part's thesis and the Markdown that follows it.
struct PartText<'a> {
    thesis: Cow<'a, str>,
    body: Cow<'a, str>,
}

impl<'a> PartText<'a> {
    /// The text of `markdown` saved without a thesis: its first paragraph stands in for the
    /// thesis, on one line, and is left out of the body so that it does not show twice. Without
    /// a paragraph of its own (outside lists and quotes), its first line of text stands in and
    /// the body keeps everything.
    fn of(markdown: &'a str) -> Self {
        let Lead { thesis, paragraph } = Lead::of(markdown);
        let Some(paragraph) = paragraph else {
            return Self {
                thesis,
                body: Cow::Borrowed(markdown),
            };
        };
        let before = markdown[..paragraph.start].trim();
        let after = markdown[paragraph.end..].trim();
        let body = match (before.is_empty(), after.is_empty()) {
            (true, _) => Cow::Borrowed(after),
            (false, true) => Cow::Borrowed(before),
            (false, false) => Cow::Owned(format!("{before}\n\n{after}")),
        };
        Self { thesis, body }
    }
}

/// A part saved before theses existed is saved as it was then, one Markdown string, so that its
/// round is written back unchanged.
impl Serialize for DesignPart {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let Some(thesis) = &self.thesis else {
            return serializer.serialize_str(&self.body);
        };
        let mut part = serializer.serialize_struct("DesignPart", 2)?;
        part.serialize_field("thesis", thesis)?;
        part.serialize_field("body", &self.body)?;
        part.end()
    }
}

/// A part saved before theses existed takes the agent's text as it was then: one Markdown
/// string with no thesis. The agent's tool input is always an object.
impl<'de> Deserialize<'de> for DesignPart {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Part {
            thesis: Option<String>,
            body: String,
        }

        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = DesignPart;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a design part with a thesis and a body")
            }

            fn visit_str<E: serde::de::Error>(self, body: &str) -> Result<DesignPart, E> {
                Ok(DesignPart {
                    thesis: None,
                    body: body.to_owned(),
                })
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                map: A,
            ) -> Result<DesignPart, A::Error> {
                let Part { thesis, body } =
                    Part::deserialize(serde::de::value::MapAccessDeserializer::new(map))?;
                Ok(DesignPart { thesis, body })
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

/// The text that stands in for the thesis of a part saved without one, and where it lies in its
/// Markdown.
struct Lead<'a> {
    /// The first paragraph that is a block of its own, on one line, or, without one, the first
    /// line of text.
    thesis: Cow<'a, str>,
    /// That paragraph, which the body then leaves out.
    paragraph: Option<Range<usize>>,
}

impl<'a> Lead<'a> {
    /// A paragraph inside a list or a quote is never taken: its markers would come with it,
    /// and taking it out would break its block.
    fn of(markdown: &'a str) -> Self {
        let options =
            Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
        let mut depth = 0usize;
        let mut first_text = None;
        for (event, range) in Parser::new_ext(markdown, options).into_offset_iter() {
            match event {
                Event::Start(Tag::Paragraph) if depth == 0 => {
                    let paragraph = &markdown[Self::trimmed(markdown, range.clone())];
                    let thesis = if paragraph.contains('\n') {
                        // A hard break, a backslash at the end of a line, becomes a space too.
                        let lines = paragraph.replace("\\\n", "\n");
                        Cow::Owned(lines.split_whitespace().collect::<Vec<_>>().join(" "))
                    } else {
                        Cow::Borrowed(paragraph)
                    };
                    return Self {
                        thesis,
                        paragraph: Some(range),
                    };
                }
                // The first line of text starts with the first inline element, as written.
                Event::Start(
                    Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link { .. },
                ) if first_text.is_none() => {
                    first_text = Some(Self::first_line(markdown, range.start..markdown.len()));
                    depth += 1;
                }
                Event::Start(_) => depth += 1,
                Event::End(_) => depth = depth.saturating_sub(1),
                Event::Text(_) | Event::Code(_) if first_text.is_none() => {
                    // A backslash escape is not part of the text it escapes.
                    let start = if markdown[..range.start].ends_with('\\') {
                        range.start - 1
                    } else {
                        range.start
                    };
                    first_text = Some(Self::first_line(markdown, start..markdown.len()));
                }
                _ => {}
            }
        }
        let thesis = first_text.unwrap_or_else(|| Self::first_line(markdown, 0..markdown.len()));
        Self {
            thesis: Cow::Borrowed(&markdown[thesis]),
            paragraph: None,
        }
    }

    /// `range` of `text` without its surrounding whitespace.
    fn trimmed(text: &str, range: Range<usize>) -> Range<usize> {
        let slice = &text[range.clone()];
        let start = range.start + (slice.len() - slice.trim_start().len());
        start..start + slice.trim().len()
    }

    /// The first line of `range` in `text` that is not blank, trimmed.
    fn first_line(text: &str, range: Range<usize>) -> Range<usize> {
        let mut start = range.start;
        for line in text[range].split_inclusive('\n') {
            if !line.trim().is_empty() {
                return Self::trimmed(text, start..start + line.len());
            }
            start += line.len();
        }
        start..start
    }
}

impl Exploration {
    /// The design the round's first turn explained, if it did.
    pub fn design(&self) -> Option<&Design> {
        let first = self.conversation.first()?;
        if first.answer.is_some() {
            return None;
        }
        first.update.design.as_ref()
    }
}

#[cfg(test)]
#[path = "design.tests.rs"]
mod tests;
