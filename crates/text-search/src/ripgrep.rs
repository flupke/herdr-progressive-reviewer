use std::io;

use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkMatch};

use crate::{Document, Match, Query, Request, Results};

pub(super) struct Engine {
    searcher: Searcher,
}

impl Default for Engine {
    fn default() -> Self {
        Self {
            searcher: SearcherBuilder::new()
                .line_number(false)
                .binary_detection(BinaryDetection::none())
                // The input is already UTF-8; preserve BOM bytes in columns.
                .bom_sniffing(false)
                .build(),
        }
    }
}

impl Engine {
    pub(super) fn search(
        &mut self,
        request: &Request,
        cancelled: impl Fn() -> bool,
    ) -> Option<Results> {
        if cancelled() {
            return None;
        }
        // A presented row can contain newlines, but queries must not cross rows.
        if request.query.contains('\n') {
            return request.search_unless(cancelled);
        }
        let query = Query::new(&request.query);
        let mut matches = Vec::new();
        if let Some(pattern) = &query.pattern {
            for (document_index, document) in request.documents.iter().enumerate() {
                if cancelled() {
                    return None;
                }
                let sink = MatchSink {
                    document,
                    document_index,
                    query: &query,
                    cancelled: &cancelled,
                    matches: &mut matches,
                };
                self.searcher
                    .search_slice(pattern, document.text.as_bytes(), sink)
                    .expect("in-memory search uses compatible line terminators");
            }
        }
        (!cancelled()).then_some(Results {
            id: request.id,
            matches,
        })
    }
}

struct MatchSink<'a, F> {
    document: &'a Document,
    document_index: usize,
    query: &'a Query,
    cancelled: &'a F,
    matches: &'a mut Vec<Match>,
}

impl<F: Fn() -> bool> Sink for MatchSink<'_, F> {
    type Error = io::Error;

    fn matched(&mut self, _: &Searcher, line: &SinkMatch<'_>) -> io::Result<bool> {
        for found in self.query.byte_ranges(line.bytes()) {
            if (self.cancelled)() {
                return Ok(false);
            }
            let offset = usize::try_from(line.absolute_byte_offset())
                .expect("slice offsets fit usize")
                + found.start;
            if let Some(position) = self.document.position(offset) {
                self.matches.push(Match {
                    document_index: self.document_index,
                    position,
                });
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
#[path = "ripgrep.tests.rs"]
mod tests;
