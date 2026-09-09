use super::RepoPath;

pub(super) struct GitPath<'a> {
    prefix: &'static str,
    path: &'a RepoPath,
    quote: bool,
}

impl<'a> GitPath<'a> {
    pub(super) fn new(prefix: &'static str, path: &'a RepoPath) -> Self {
        let quote = !path
            .as_bytes()
            .iter()
            .all(|byte| matches!(byte, b'!'..=b'~') && !matches!(byte, b'"' | b'\\'));
        Self {
            prefix,
            path,
            quote,
        }
    }

    pub(super) fn with_quoting(mut self, quote: bool) -> Self {
        self.quote = quote;
        self
    }

    pub(super) fn append_to(&self, output: &mut Vec<u8>) {
        if self.quote {
            output.push(b'"');
        }
        output.extend_from_slice(self.prefix.as_bytes());
        for byte in self.path.as_bytes() {
            if self.quote {
                match byte {
                    b'\\' | b'"' => output.extend_from_slice(&[b'\\', *byte]),
                    b'\t' => output.extend_from_slice(br"\t"),
                    b'\n' => output.extend_from_slice(br"\n"),
                    b'\r' => output.extend_from_slice(br"\r"),
                    b' '..=b'~' => output.push(*byte),
                    _ => output.extend_from_slice(&[
                        b'\\',
                        b'0' + (byte >> 6),
                        b'0' + ((byte >> 3) & 7),
                        b'0' + (byte & 7),
                    ]),
                }
            } else {
                output.push(*byte);
            }
        }
        if self.quote {
            output.push(b'"');
        }
    }
}

impl std::fmt::Display for GitPath<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut bytes = Vec::new();
        self.append_to(&mut bytes);
        let text = std::str::from_utf8(&bytes).map_err(|_| std::fmt::Error)?;
        formatter.write_str(text)
    }
}
