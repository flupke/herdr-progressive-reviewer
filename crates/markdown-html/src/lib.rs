//! Markdown written by an agent, as HTML a page can show.
//!
//! The HTML holds standard Markdown with tables, strikethrough and task lists, and the marks of
//! [`markdown_marks`]: a quote that opens with a callout marker becomes a `role="note"` block of
//! class `callout callout-<kind>`, and a table cell that opens with a status marker gets the
//! class `status-<status>` and a `role="img"` mark in place of the marker. A fenced block keeps
//! its language as the class `language-<name>` of its `<code>`, so a script can find the
//! blocks of one language.
//!
//! The agent's text is not trusted: raw HTML shows as text, only `http`, `https` and `mailto`
//! links stay links, images show their description, and nothing carries an inline style, which
//! the page's content security policy refuses.

use markdown_marks::{Callout, Mark, StatusMark};
use pulldown_cmark::{
    Alignment, CowStr, Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd,
};

#[cfg(test)]
mod tests;

/// Renders Markdown as [`Html`].
#[derive(Clone, Copy, Debug)]
pub struct HtmlRenderer {
    /// How many levels the text's headings move down.
    heading_shift: usize,
}

impl HtmlRenderer {
    /// For text that sits under a heading of `level`: its `#` headings come one level below,
    /// and none goes deeper than `<h6>`.
    pub fn under_heading(level: usize) -> Self {
        Self {
            heading_shift: level,
        }
    }

    pub fn render(self, markdown: &str) -> Html {
        let options =
            Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
        let events = pulldown_cmark::TextMergeStream::new(Parser::new_ext(markdown, options));
        let mut rewrite = Rewrite::new(self, events.collect());
        rewrite.run();
        let mut html = String::new();
        pulldown_cmark::html::push_html(&mut html, rewrite.output.into_iter());
        Html(html)
    }
}

/// HTML rendered from an agent's Markdown, safe to put in a page as it is.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Html(String);

impl Html {
    pub fn into_string(self) -> String {
        self.0
    }
}

/// Rewrites the parser's events into the events of the page's HTML.
struct Rewrite<'a> {
    renderer: HtmlRenderer,
    /// The parser's events, in reverse, so the next one pops off the end.
    input: Vec<Event<'a>>,
    output: Vec<Event<'a>>,
    /// For each quote open, whether it is a callout.
    quotes: Vec<bool>,
    /// For each link open, whether it stays a link.
    links: Vec<bool>,
    table: TableState,
}

/// Where the rewrite is in a table: the cells take their alignment and element from it.
#[derive(Default)]
struct TableState {
    alignments: Vec<Alignment>,
    in_head: bool,
    column: usize,
}

impl TableState {
    fn start(&mut self, tag: &Tag<'_>) {
        match tag {
            Tag::Table(alignments) => self.alignments.clone_from(alignments),
            Tag::TableHead => {
                self.in_head = true;
                self.column = 0;
            }
            Tag::TableRow => self.column = 0,
            _ => {}
        }
    }
}

impl<'a> Rewrite<'a> {
    fn new(renderer: HtmlRenderer, mut input: Vec<Event<'a>>) -> Self {
        input.reverse();
        Self {
            renderer,
            input,
            output: Vec::new(),
            quotes: Vec::new(),
            links: Vec::new(),
            table: TableState::default(),
        }
    }

    fn run(&mut self) {
        while let Some(event) = self.input.pop() {
            match event {
                Event::Start(tag) => self.start(tag),
                Event::End(tag) => self.end(tag),
                Event::Html(html) | Event::InlineHtml(html) => self.output.push(Event::Text(html)),
                event => self.output.push(event),
            }
        }
    }

    /// The event `ahead` events after the next one (0 for the next), without taking it.
    fn peek(&self, ahead: usize) -> Option<&Event<'a>> {
        self.input
            .len()
            .checked_sub(ahead + 1)
            .map(|index| &self.input[index])
    }

    /// The mark whose marker opens the text event `ahead` (see [`Self::peek`]), and the text
    /// after the marker.
    fn marked<M: Mark>(&self, ahead: usize) -> Option<(M, CowStr<'a>)> {
        match self.peek(ahead) {
            Some(Event::Text(text)) => {
                M::strip(text).map(|(mark, rest)| (mark, CowStr::from(rest.to_owned())))
            }
            _ => None,
        }
    }

    fn start(&mut self, tag: Tag<'a>) {
        let tag = match tag {
            Tag::HtmlBlock => Tag::Paragraph,
            Tag::BlockQuote(kind) => return self.quote(Tag::BlockQuote(kind)),
            tag @ (Tag::Table(_) | Tag::TableHead | Tag::TableRow) => {
                self.table.start(&tag);
                tag
            }
            Tag::TableCell => return self.cell(),
            Tag::Heading {
                level,
                id,
                classes,
                attrs,
            } => Tag::Heading {
                level: self.shift(level),
                id,
                classes,
                attrs,
            },
            Tag::Link {
                link_type,
                ref dest_url,
                ..
            } => {
                let kept = link_type == LinkType::Email || is_web_link(dest_url);
                self.links.push(kept);
                if !kept {
                    return;
                }
                tag
            }
            Tag::Image { .. } => return,
            tag => tag,
        };
        self.output.push(Event::Start(tag));
    }

    fn end(&mut self, tag: TagEnd) {
        let tag = match tag {
            TagEnd::HtmlBlock => TagEnd::Paragraph,
            TagEnd::BlockQuote(kind) => {
                if self.quotes.pop() == Some(true) {
                    self.output.push(Event::Html("</div>\n".into()));
                    return;
                }
                TagEnd::BlockQuote(kind)
            }
            TagEnd::TableHead => {
                self.table.in_head = false;
                TagEnd::TableHead
            }
            TagEnd::TableCell => {
                self.table.column += 1;
                TagEnd::TableCell
            }
            TagEnd::Heading(level) => TagEnd::Heading(self.shift(level)),
            TagEnd::Link => {
                if self.links.pop() != Some(true) {
                    return;
                }
                TagEnd::Link
            }
            TagEnd::Image => return,
            tag => tag,
        };
        self.output.push(Event::End(tag));
    }

    fn shift(&self, level: HeadingLevel) -> HeadingLevel {
        let shifted = (level as usize + self.renderer.heading_shift).min(6);
        HeadingLevel::try_from(shifted).unwrap_or(HeadingLevel::H6)
    }

    /// A quote whose first paragraph opens with a callout marker becomes that callout, titled
    /// in place of the marker.
    fn quote(&mut self, tag: Tag<'a>) {
        let callout = matches!(self.peek(0), Some(Event::Start(Tag::Paragraph)))
            .then(|| self.marked::<Callout>(1))
            .flatten();
        self.quotes.push(callout.is_some());
        let Some((callout, rest)) = callout else {
            self.output.push(Event::Start(tag));
            return;
        };
        self.output.push(Event::Html(
            format!(
                "<div class=\"callout callout-{name}\" role=\"note\" aria-label=\"{label}\">\n\
                 <p class=\"callout-title\">{label}</p>\n",
                name = callout.name(),
                label = callout.label(),
            )
            .into(),
        ));
        let paragraph = self.input.pop();
        self.input.pop();
        if !rest.is_empty() {
            self.output.extend(paragraph);
            self.output.push(Event::Text(rest));
            return;
        }
        // The marker stood alone: drop its line, and its paragraph when nothing else was in it.
        match self.peek(0) {
            Some(Event::SoftBreak | Event::HardBreak) => {
                self.input.pop();
                self.output.extend(paragraph);
            }
            Some(Event::End(TagEnd::Paragraph)) => {
                self.input.pop();
            }
            _ => self.output.extend(paragraph),
        }
    }

    /// A table cell, aligned by a class, with the status its marker gives.
    fn cell(&mut self) {
        let element = if self.table.in_head { "th" } else { "td" };
        let mut classes = Vec::new();
        match self.table.alignments.get(self.table.column) {
            Some(Alignment::Left) => classes.push("align-left"),
            Some(Alignment::Center) => classes.push("align-center"),
            Some(Alignment::Right) => classes.push("align-right"),
            Some(Alignment::None) | None => {}
        }
        let status = self.marked::<StatusMark>(0);
        let status_class = status
            .as_ref()
            .map(|(mark, _)| format!("status-{}", mark.name()));
        classes.extend(status_class.as_deref());
        let class = if classes.is_empty() {
            String::new()
        } else {
            format!(" class=\"{}\"", classes.join(" "))
        };
        self.output
            .push(Event::Html(format!("<{element}{class}>").into()));
        let Some((mark, rest)) = status else {
            return;
        };
        self.input.pop();
        self.output.push(Event::Html(
            format!(
                "<span class=\"status\" role=\"img\" aria-label=\"{}\">{}</span>",
                mark.label(),
                mark.symbol()
            )
            .into(),
        ));
        if !rest.is_empty() {
            self.output.push(Event::Text(format!(" {rest}").into()));
        } else if !matches!(self.peek(0), Some(Event::End(TagEnd::TableCell))) {
            // Inline markup follows the marker.
            self.output.push(Event::Text(" ".into()));
        }
    }
}

/// Whether a link to `url` stays a link: only web and `mailto:` addresses do.
fn is_web_link(url: &str) -> bool {
    ["http://", "https://", "mailto:"].iter().any(|scheme| {
        url.get(..scheme.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(scheme))
    })
}
