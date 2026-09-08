//! Jujutsu change snapshots.

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::Path;

use super::jj_git_diff::{DESCRIPTION_DIFF_PATHS, JjGitDiffParser};
use super::{
    BaselineComparison, BaselineComparisonPlan, BaselineComparisonResults, ChangeId, ChangedFile,
    Interdiff, RepoPath, Repository, RepositoryBackend, RevisionCandidate, RevisionDirection,
    RevisionHistoryLine, Snapshot, SnapshotIdentity,
};
use crate::{Error, Result};

pub(super) const IDENTITY_TEMPLATE: &str = r#"change_id ++ "\0" ++ commit_id ++ "\0" ++ description ++ "\0" ++ change_id.shortest(8) ++ "\0""#;
const FILE_TEMPLATE: &str = concat!(
    r#"source.path() ++ "\0" ++ target.path() ++ "\0" ++ "#,
    r#"source.file_type() ++ "\0" ++ target.file_type() ++ "\0" ++ "#,
    r#"status ++ "\0""#,
);
const STATS_TEMPLATE: &str = concat!(
    r#"diff.stat().files().map(|entry| entry.path() ++ "\0" ++ "#,
    r#"entry.lines_added() ++ "\0" ++ entry.lines_removed() ++ "\0").join("")"#,
);
const DESCRIPTION_DIFF_HEADER: &[u8] =
    b"diff --git a/JJ-COMMIT-DESCRIPTION b/JJ-COMMIT-DESCRIPTION\n";
const NEXT_DIFF_HEADER: &[u8] = b"\ndiff --git ";
const REVISION_CANDIDATE_TEMPLATE: &str = concat!(
    r#"change_id ++ "\0" ++ change_id.shortest(8) ++ "\0" ++ "#,
    r#"description.first_line() ++ "\0""#,
);
const REVISION_HISTORY_REVSET: &str = "(descendants(heads(ancestors(@) & immutable())) & mutable())
    | heads(ancestors(@) & immutable())
    | (children(heads(ancestors(@) & immutable())) & immutable())";
const REVISION_HISTORY_TEMPLATE: &str = concat!(
    r#""\x1e" ++ change_id ++ ":" ++ change_id.shortest(8) ++ ":" ++ if(current_working_copy, "1", "0") ++ ":" ++ if(immutable, "1", "0") ++ "\x1f" ++ "\x1d" ++ "#,
    r#"change_id.shortest(8) ++ " " ++ description.first_line() ++ "\n""#,
);

#[derive(Debug)]
pub(super) struct JjBackend;

impl RepositoryBackend for JjBackend {
    fn set_state_root(&self, _repository_root: &Path, _state_root: &Path) {}

    fn current_identity(&self, repository: &Repository) -> Result<SnapshotIdentity> {
        repository.read_jj_identity(false)
    }

    fn read_files(
        &self,
        repository: &Repository,
        identity: &SnapshotIdentity,
    ) -> Result<Vec<ChangedFile>> {
        ChangedFile::parse_all(
            &repository
                .run_jj([
                    OsString::from("diff"),
                    OsString::from("-r"),
                    OsString::from(identity.snapshot_id()),
                    OsString::from("-T"),
                    OsString::from(FILE_TEMPLATE),
                ])?
                .stdout,
        )
    }

    fn read_stats(
        &self,
        repository: &Repository,
        identity: &SnapshotIdentity,
        files: &mut [ChangedFile],
    ) -> Result<()> {
        ChangedFile::add_stats(
            files,
            &repository
                .run_jj([
                    OsString::from("--ignore-working-copy"),
                    OsString::from("log"),
                    OsString::from("--no-graph"),
                    OsString::from("-r"),
                    OsString::from(identity.snapshot_id()),
                    OsString::from("-T"),
                    OsString::from(STATS_TEMPLATE),
                ])?
                .stdout,
        )
    }

    fn diff(
        &self,
        repository: &Repository,
        snapshot: &Snapshot,
        file: &ChangedFile,
    ) -> Result<Vec<u8>> {
        let mut arguments = vec![
            OsString::from("diff"),
            OsString::from("-r"),
            OsString::from(snapshot.identity.snapshot_id()),
            OsString::from("--git"),
            OsString::from("--"),
        ];
        arguments.extend(file.diff_paths().map(|path| path.as_os_str().to_owned()));
        Ok(repository.run_jj(arguments)?.stdout)
    }

    fn file_at(&self, repository: &Repository, revision: &str, path: &RepoPath) -> Result<Vec<u8>> {
        Ok(repository
            .run_jj([
                OsString::from("--ignore-working-copy"),
                OsString::from("file"),
                OsString::from("show"),
                OsString::from("-r"),
                OsString::from(revision),
                OsString::from("--"),
                path.as_os_str().to_owned(),
            ])?
            .stdout)
    }

    fn base_file_at(
        &self,
        repository: &Repository,
        snapshot: &Snapshot,
        path: &RepoPath,
    ) -> Result<Vec<u8>> {
        self.file_at(
            repository,
            &format!("{}-", snapshot.identity.snapshot_id()),
            path,
        )
    }

    fn interdiff(
        &self,
        repository: &Repository,
        baseline_snapshot_id: &str,
        snapshot: &Snapshot,
        path: &RepoPath,
    ) -> Result<Interdiff> {
        let baseline = repository.output_jj([
            OsString::from("--ignore-working-copy"),
            OsString::from("log"),
            OsString::from("--no-graph"),
            OsString::from("-r"),
            OsString::from(baseline_snapshot_id),
            OsString::from("-T"),
            OsString::from(r#"commit_id ++ "\n""#),
        ])?;
        if !baseline.status.success() {
            return Ok(Interdiff::MissingBaseline);
        }
        Ok(Interdiff::Diff(strip_description_diff(
            repository
                .run_jj([
                    OsString::from("--ignore-working-copy"),
                    OsString::from("interdiff"),
                    OsString::from("--from"),
                    OsString::from(baseline_snapshot_id),
                    OsString::from("--to"),
                    OsString::from(snapshot.identity.snapshot_id()),
                    OsString::from("--git"),
                    OsString::from("--"),
                    path.as_os_str().to_owned(),
                ])?
                .stdout,
        )))
    }

    fn compare_baselines(
        &self,
        repository: &Repository,
        snapshot: &Snapshot,
        plan: &BaselineComparisonPlan,
    ) -> Result<BaselineComparisonResults> {
        let mut results = BaselineComparisonResults::default();
        for (baseline, paths) in plan.baselines() {
            let mut arguments = vec![
                OsString::from("--ignore-working-copy"),
                OsString::from("interdiff"),
                OsString::from("--from"),
                OsString::from(baseline.as_str()),
                OsString::from("--to"),
                OsString::from(snapshot.identity.snapshot_id()),
                OsString::from("--git"),
                OsString::from("--"),
            ];
            arguments.extend(paths.iter().map(jj_exact_fileset));
            let output = repository.output_jj(arguments)?;
            if !output.status.success() {
                if !baseline_exists(repository, baseline.as_str())? {
                    results.insert(baseline.clone(), BaselineComparison::Missing);
                    continue;
                }
                return Err(Error::CommandFailed {
                    operation: "read jj repository".to_owned(),
                    code: output.status.code(),
                });
            }

            let path_statistics = JjGitDiffParser::new(&output.stdout, paths).parse()?;
            results.insert(
                baseline.clone(),
                BaselineComparison::Compared { path_statistics },
            );
        }
        Ok(results)
    }
}

fn baseline_exists(repository: &Repository, baseline: &str) -> Result<bool> {
    Ok(repository
        .output_jj([
            "--ignore-working-copy",
            "log",
            "--no-graph",
            "-r",
            baseline,
            "-T",
            r#"commit_id ++ "\n""#,
        ])?
        .status
        .success())
}

fn jj_exact_fileset(path: &RepoPath) -> OsString {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let mut fileset = b"root-file:\"".to_vec();
    for byte in path.as_bytes() {
        match byte {
            b'"' => fileset.extend_from_slice(br#"\""#),
            b'\\' => fileset.extend_from_slice(br"\\"),
            b' '..=b'~' => fileset.push(*byte),
            _ => fileset.extend_from_slice(&[
                b'\\',
                b'x',
                HEX[usize::from(byte >> 4)],
                HEX[usize::from(byte & 0x0f)],
            ]),
        }
    }
    fileset.push(b'"');
    OsString::from_vec(fileset)
}

fn strip_description_diff(mut diff: Vec<u8>) -> Vec<u8> {
    if !diff.starts_with(DESCRIPTION_DIFF_HEADER) {
        return diff;
    }

    let block_end = diff[DESCRIPTION_DIFF_HEADER.len()..]
        .windows(NEXT_DIFF_HEADER.len())
        .position(|window| window == NEXT_DIFF_HEADER)
        .map_or(diff.len(), |index| {
            index + DESCRIPTION_DIFF_HEADER.len() + 1
        });
    if diff[..block_end]
        .windows(DESCRIPTION_DIFF_PATHS.len())
        .any(|window| window == DESCRIPTION_DIFF_PATHS)
    {
        diff.drain(..block_end);
    }
    diff
}

pub(super) fn revision_candidates(
    repository: &Repository,
    direction: RevisionDirection,
) -> Result<Vec<RevisionCandidate>> {
    let revset = match direction {
        RevisionDirection::Parents => "parents(@) & mutable()",
        RevisionDirection::Children => "children(@) & mutable()",
    };
    let output = repository.run_jj([
        "--ignore-working-copy",
        "log",
        "--no-graph",
        "-r",
        revset,
        "-T",
        REVISION_CANDIDATE_TEMPLATE,
    ])?;
    parse_revision_candidates(&output.stdout)
}

pub(super) fn revision_history(repository: &Repository) -> Result<Vec<RevisionHistoryLine>> {
    let output = repository.run_jj([
        "--ignore-working-copy",
        "--color=always",
        "log",
        "-r",
        REVISION_HISTORY_REVSET,
        "-T",
        REVISION_HISTORY_TEMPLATE,
    ])?;
    parse_revision_history(&output.stdout)
}

fn parse_revision_history(output: &[u8]) -> Result<Vec<RevisionHistoryLine>> {
    output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(parse_revision_history_line)
        .collect()
}

fn parse_revision_history_line(line: &[u8]) -> Result<RevisionHistoryLine> {
    let record_start = line.iter().position(|byte| *byte == 0x1e);
    let record_end = record_start.and_then(|start| {
        line[start + 1..]
            .iter()
            .position(|byte| *byte == 0x1f)
            .map(|offset| start + offset + 1)
    });
    let display_start = record_end.and_then(|end| {
        line[end + 1..]
            .iter()
            .position(|byte| *byte == 0x1d)
            .map(|offset| end + offset + 2)
    });
    let (text, short_change_id, change_id, is_current, is_immutable) =
        match (record_start, record_end, display_start) {
            (Some(start), Some(end), Some(display_start)) => {
                let metadata = strip_ansi_escapes::strip(&line[start + 1..end]);
                let mut fields = metadata.rsplitn(4, |byte| *byte == b':');
                let immutable_marker = fields.next();
                let current_marker = fields.next();
                let short_change_id = fields.next();
                let change_id = fields.next();
                let (
                    Some(immutable_marker),
                    Some(current_marker),
                    Some(short_change_id),
                    Some(change_id),
                ) = (immutable_marker, current_marker, short_change_id, change_id)
                else {
                    return Err(Error::Protocol {
                        operation: "read jj revision history".to_owned(),
                        detail: "jj returned incomplete revision history metadata",
                    });
                };
                let change_id = std::str::from_utf8(change_id).map_err(|_| Error::Protocol {
                    operation: "read jj revision history".to_owned(),
                    detail: "jj returned a non-UTF-8 revision identifier",
                })?;
                let short_change_id =
                    std::str::from_utf8(short_change_id).map_err(|_| Error::Protocol {
                        operation: "read jj revision history".to_owned(),
                        detail: "jj returned a non-UTF-8 short revision identifier",
                    })?;
                let is_current = parse_revision_history_marker(
                    current_marker,
                    "jj returned an invalid current revision marker",
                )?;
                let is_immutable = parse_revision_history_marker(
                    immutable_marker,
                    "jj returned an invalid immutable revision marker",
                )?;
                let mut text = line[..start].to_vec();
                text.extend_from_slice(&line[display_start..]);
                (
                    text,
                    Some(short_change_id.to_owned()),
                    Some(ChangeId::from(change_id.to_owned())),
                    is_current,
                    is_immutable,
                )
            }
            (None, None, None) => (line.to_vec(), None, None, false, false),
            _ => {
                return Err(Error::Protocol {
                    operation: "read jj revision history".to_owned(),
                    detail: "jj returned an incomplete revision history record",
                });
            }
        };
    let plain_text = strip_ansi_escapes::strip(&text);
    Ok(RevisionHistoryLine {
        text: String::from_utf8(text).map_err(|_| Error::Protocol {
            operation: "read jj revision history".to_owned(),
            detail: "jj returned non-UTF-8 revision history text",
        })?,
        plain_text: String::from_utf8(plain_text).map_err(|_| Error::Protocol {
            operation: "read jj revision history".to_owned(),
            detail: "jj returned non-UTF-8 plain revision history text",
        })?,
        short_change_id,
        change_id,
        is_current,
        is_immutable,
    })
}

fn parse_revision_history_marker(marker: &[u8], invalid_detail: &'static str) -> Result<bool> {
    match marker {
        b"0" => Ok(false),
        b"1" => Ok(true),
        _ => Err(Error::Protocol {
            operation: "read jj revision history".to_owned(),
            detail: invalid_detail,
        }),
    }
}

fn parse_revision_candidates(output: &[u8]) -> Result<Vec<RevisionCandidate>> {
    if output.is_empty() {
        return Ok(Vec::new());
    }
    let fields = output.split(|byte| *byte == 0).collect::<Vec<_>>();
    if fields.last() != Some(&&[][..]) || (fields.len() - 1) % 3 != 0 {
        return Err(Error::Protocol {
            operation: "read jj revision candidates".to_owned(),
            detail: "jj returned an invalid revision candidate record",
        });
    }
    fields[..fields.len() - 1]
        .chunks_exact(3)
        .map(|fields| {
            let parse = |value: &[u8]| {
                std::str::from_utf8(value)
                    .map(str::to_owned)
                    .map_err(|_| Error::Protocol {
                        operation: "read jj revision candidates".to_owned(),
                        detail: "jj returned non-UTF-8 revision candidate text",
                    })
            };
            Ok(RevisionCandidate {
                change_id: ChangeId::from(parse(fields[0])?),
                short_change_id: parse(fields[1])?,
                description: parse(fields[2])?,
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "jj.tests.rs"]
mod tests;
