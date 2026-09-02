use std::collections::{BTreeMap, BTreeSet};

use super::{DiffStatistics, RepoPath};
use crate::{Error, Result};

pub(super) const DESCRIPTION_DIFF_PATHS: &[u8] =
    b"--- JJ-COMMIT-DESCRIPTION\n+++ JJ-COMMIT-DESCRIPTION\n";
const DIFF_HEADER: &[u8] = b"diff --git ";
const PATH_MARKERS: &[(&[u8], Option<&[u8]>)] = &[
    (b"--- ", Some(b"a/")),
    (b"+++ ", Some(b"b/")),
    (b"rename from ", None),
    (b"rename to ", None),
    (b"copy from ", None),
    (b"copy to ", None),
];

pub(super) struct JjGitDiffParser<'a> {
    output: &'a [u8],
    planned_paths: &'a BTreeSet<RepoPath>,
}

impl<'a> JjGitDiffParser<'a> {
    pub(super) fn new(output: &'a [u8], planned_paths: &'a BTreeSet<RepoPath>) -> Self {
        Self {
            output,
            planned_paths,
        }
    }

    pub(super) fn parse(&self) -> Result<BTreeMap<RepoPath, DiffStatistics>> {
        let mut path_statistics = BTreeMap::new();
        let block_starts = self.block_starts();
        if block_starts.is_empty() {
            return if self.output.is_empty() {
                Ok(path_statistics)
            } else {
                Err(Self::invalid_diff(
                    "jj returned text outside a Git diff block",
                ))
            };
        }

        for (block_index, start) in block_starts.iter().copied().enumerate() {
            let end = block_starts
                .get(block_index + 1)
                .copied()
                .unwrap_or(self.output.len());
            self.parse_block(&self.output[start..end], &mut path_statistics)?;
        }
        Ok(path_statistics)
    }

    fn block_starts(&self) -> Vec<usize> {
        self.output
            .windows(DIFF_HEADER.len())
            .enumerate()
            .filter_map(|(index, window)| {
                (window == DIFF_HEADER && (index == 0 || self.output[index - 1] == b'\n'))
                    .then_some(index)
            })
            .collect()
    }

    fn parse_block(
        &self,
        block: &[u8],
        path_statistics: &mut BTreeMap<RepoPath, DiffStatistics>,
    ) -> Result<()> {
        if block
            .windows(DESCRIPTION_DIFF_PATHS.len())
            .any(|window| window == DESCRIPTION_DIFF_PATHS)
        {
            return Ok(());
        }

        let statistics = DiffStatistics::from_unified_diff(block);
        let mut matched = false;
        for line in block.split(|byte| *byte == b'\n') {
            let Some(path_bytes) = Self::path_from_marker_line(line)? else {
                continue;
            };
            if let Some(path) = self.planned_path(&path_bytes) {
                path_statistics.insert(path.clone(), statistics);
                matched = true;
            }
        }
        if !matched {
            matched = self.match_mode_only_header(block, path_statistics);
        }
        if !matched {
            return Err(Self::invalid_diff(
                "jj returned a path outside the comparison plan",
            ));
        }
        Ok(())
    }

    fn path_from_marker_line(line: &[u8]) -> Result<Option<Vec<u8>>> {
        for &(marker, required_prefix) in PATH_MARKERS {
            let Some(encoded_path) = line.strip_prefix(marker) else {
                continue;
            };
            if encoded_path == b"/dev/null" {
                return Ok(None);
            }
            let decoded_path = Self::decode_path(encoded_path)?;
            let path = match required_prefix {
                Some(prefix) => decoded_path
                    .strip_prefix(prefix)
                    .ok_or_else(|| Self::invalid_diff("jj returned an invalid Git path marker"))?,
                None => decoded_path.as_slice(),
            };
            return Ok(Some(path.to_vec()));
        }
        Ok(None)
    }

    fn match_mode_only_header(
        &self,
        block: &[u8],
        path_statistics: &mut BTreeMap<RepoPath, DiffStatistics>,
    ) -> bool {
        let header_end = block
            .iter()
            .position(|byte| *byte == b'\n')
            .unwrap_or(block.len());
        let header = &block[..header_end];
        self.planned_paths.iter().any(|path| {
            if Self::same_path_header(path, false) == header
                || Self::same_path_header(path, true) == header
            {
                path_statistics.insert(path.clone(), DiffStatistics::default());
                true
            } else {
                false
            }
        })
    }

    fn same_path_header(path: &RepoPath, quote_paths: bool) -> Vec<u8> {
        let mut header = DIFF_HEADER.to_vec();
        Self::append_header_path(&mut header, b'a', path, quote_paths);
        header.push(b' ');
        Self::append_header_path(&mut header, b'b', path, quote_paths);
        header
    }

    fn append_header_path(header: &mut Vec<u8>, prefix: u8, path: &RepoPath, quote: bool) {
        if quote {
            header.push(b'"');
        }
        header.extend_from_slice(&[prefix, b'/']);
        for byte in path.as_bytes() {
            if quote {
                match byte {
                    b'\\' | b'"' => header.extend_from_slice(&[b'\\', *byte]),
                    b'\t' => header.extend_from_slice(br"\t"),
                    b'\n' => header.extend_from_slice(br"\n"),
                    b'\r' => header.extend_from_slice(br"\r"),
                    b' '..=b'~' => header.push(*byte),
                    _ => header.extend_from_slice(&[
                        b'\\',
                        b'0' + (byte >> 6),
                        b'0' + ((byte >> 3) & 7),
                        b'0' + (byte & 7),
                    ]),
                }
            } else {
                header.push(*byte);
            }
        }
        if quote {
            header.push(b'"');
        }
    }

    fn planned_path(&self, path_bytes: &[u8]) -> Option<&RepoPath> {
        self.planned_paths
            .iter()
            .find(|path| path.as_bytes() == path_bytes)
    }

    fn decode_path(encoded_path: &[u8]) -> Result<Vec<u8>> {
        if !encoded_path.starts_with(b"\"") {
            return Ok(encoded_path.to_vec());
        }
        if !encoded_path.ends_with(b"\"") {
            return Err(Self::invalid_path());
        }

        let mut decoded = Vec::with_capacity(encoded_path.len() - 2);
        let mut bytes = encoded_path[1..encoded_path.len() - 1].iter().copied();
        while let Some(byte) = bytes.next() {
            decoded.push(if byte == b'\\' {
                Self::decode_escape(&mut bytes)?
            } else {
                byte
            });
        }
        Ok(decoded)
    }

    fn decode_escape(bytes: &mut impl Iterator<Item = u8>) -> Result<u8> {
        let escaped = bytes.next().ok_or_else(Self::invalid_path)?;
        match escaped {
            b'\\' | b'"' => Ok(escaped),
            b'a' => Ok(0x07),
            b'b' => Ok(0x08),
            b't' => Ok(b'\t'),
            b'n' => Ok(b'\n'),
            b'v' => Ok(0x0b),
            b'f' => Ok(0x0c),
            b'r' => Ok(b'\r'),
            b'0'..=b'7' => Self::decode_octal_escape(escaped, bytes),
            _ => Err(Self::invalid_path()),
        }
    }

    fn decode_octal_escape(first: u8, bytes: &mut impl Iterator<Item = u8>) -> Result<u8> {
        let second = bytes.next().ok_or_else(Self::invalid_path)?;
        let third = bytes.next().ok_or_else(Self::invalid_path)?;
        if !(b'0'..=b'7').contains(&second) || !(b'0'..=b'7').contains(&third) {
            return Err(Self::invalid_path());
        }
        let value =
            u16::from(first - b'0') * 64 + u16::from(second - b'0') * 8 + u16::from(third - b'0');
        u8::try_from(value).map_err(|_| Self::invalid_path())
    }

    fn invalid_path() -> Error {
        Self::invalid_diff("jj returned an invalid escaped Git path")
    }

    fn invalid_diff(detail: &'static str) -> Error {
        Error::Protocol {
            operation: "compare jj review baselines".to_owned(),
            detail,
        }
    }
}

#[cfg(test)]
#[path = "jj_git_diff.tests.rs"]
mod tests;
