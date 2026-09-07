use std::io::{self, BufRead, Cursor, Read};

use lsp_server::Message;

use crate::language::LanguageServer;

pub(super) struct MessageReader<R> {
    input: R,
    skip_preamble: bool,
}

impl<R: BufRead> MessageReader<R> {
    pub(super) fn new(input: R, server: LanguageServer) -> Self {
        Self {
            input,
            skip_preamble: server == LanguageServer::Expert,
        }
    }

    pub(super) fn read(&mut self) -> io::Result<Option<Message>> {
        if !self.skip_preamble {
            return Message::read(&mut self.input);
        }
        // Expert's standalone installer can print setup messages before starting LSP.
        let mut remaining = 16 * 1024;
        loop {
            let mut line = String::new();
            let read = self.input.by_ref().take(remaining).read_line(&mut line)?;
            if read == 0 {
                return Ok(None);
            }
            remaining -= read as u64;
            if line.starts_with("Content-Length:") || line.starts_with("Content-Type:") {
                self.skip_preamble = false;
                return Message::read(&mut Cursor::new(line).chain(&mut self.input));
            }
            if remaining == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Expert startup output exceeded 16 KiB",
                ));
            }
        }
    }
}

#[cfg(test)]
#[path = "reader.tests.rs"]
mod tests;
