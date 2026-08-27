use super::*;

#[test]
fn checkpoint_identity_requires_the_review_unit_and_checkpoint() {
    let checkpoint = ReviewCheckpoint::new("review-unit", "checkpoint");

    assert!(checkpoint.matches("review-unit", "checkpoint"));
    assert!(!checkpoint.matches("other-unit", "checkpoint"));
    assert!(!checkpoint.matches("review-unit", "other-checkpoint"));
}

#[test]
fn request_id_preserves_its_exact_value() {
    let request_id = GuideRequestId::new("request-123");

    assert_eq!(request_id.as_str(), "request-123");
    assert_eq!(request_id.as_bytes(), b"request-123");
    assert_eq!(request_id.to_string(), "request-123");
    assert!(!request_id.is_empty());
    assert!(GuideRequestId::new("").is_empty());
}

#[test]
fn edits_before_an_interval_shift_it() {
    assert_eq!(
        transform_interval(10..14, &[(2..2, 2..5), (20..21, 20..20)]),
        Some(13..17)
    );
}

#[test]
fn an_overlapping_edit_hides_the_interval() {
    assert_eq!(transform_interval(10..14, &[(12..13, 12..14)]), None);
}

#[test]
fn histogram_edits_preserve_missing_final_newlines() {
    assert_eq!(
        histogram_edits(b"one\ntwo", b"zero\none\ntwo"),
        vec![(0..0, 0..1)]
    );
}

#[test]
fn valid_items_are_normalized_and_overlaps_are_rejected() {
    let response = GuideResponse {
        schema_version: 1,
        request_id: "request".to_owned(),
        items: vec![
            GuideItem {
                target: GuideTarget::Hunks {
                    path: "src/lib.rs".to_owned(),
                    first_hunk: 1,
                    last_hunk: 2,
                },
                text: "  central   idea  ".to_owned(),
                status: GuideItemStatus::Stale,
            },
            GuideItem {
                target: GuideTarget::Hunks {
                    path: "src/lib.rs".to_owned(),
                    first_hunk: 2,
                    last_hunk: 2,
                },
                text: "overlap".to_owned(),
                status: GuideItemStatus::Matched,
            },
        ],
    };
    let validated = response
        .validate(
            "request",
            &[FrozenFile {
                path: "src/lib.rs".to_owned(),
                hunk_count: 2,
                old_path: None,
                new_path: None,
                old_content: None,
                new_content: None,
                hunks: vec![
                    FrozenHunk {
                        old: Some(0..1),
                        new: Some(0..1),
                    },
                    FrozenHunk {
                        old: Some(2..3),
                        new: Some(2..3),
                    },
                ],
                diff_hash: String::new(),
            }],
        )
        .unwrap();
    assert_eq!(validated.items.len(), 1);
    assert_eq!(validated.items[0].text, "central idea");
    assert_eq!(validated.items[0].status, GuideItemStatus::Matched);
    assert_eq!(validated.rejected_items, 1);
}

#[test]
fn invalid_hunk_ranges_are_rejected() {
    let hunk_item = |first_hunk, last_hunk| GuideItem {
        target: GuideTarget::Hunks {
            path: "src/lib.rs".to_owned(),
            first_hunk,
            last_hunk,
        },
        text: "central idea".to_owned(),
        status: GuideItemStatus::Matched,
    };
    let response = GuideResponse {
        schema_version: 1,
        request_id: "request".to_owned(),
        items: vec![hunk_item(0, 1), hunk_item(2, 1), hunk_item(1, 3)],
    };
    let file = FrozenFile {
        path: "src/lib.rs".to_owned(),
        hunk_count: 2,
        old_path: None,
        new_path: None,
        old_content: None,
        new_content: None,
        hunks: Vec::new(),
        diff_hash: String::new(),
    };

    let validated = response.validate("request", &[file]).unwrap();

    assert!(validated.items.is_empty());
    assert_eq!(validated.rejected_items, 3);
}

#[test]
fn one_complete_hunk_is_a_valid_target() {
    let item = GuideItem {
        target: GuideTarget::Hunks {
            path: "src/lib.rs".to_owned(),
            first_hunk: 1,
            last_hunk: 1,
        },
        text: "one operation".to_owned(),
        status: GuideItemStatus::Matched,
    };
    let response = GuideResponse {
        schema_version: 1,
        request_id: "request".to_owned(),
        items: vec![item.clone()],
    };
    let file = FrozenFile {
        path: "src/lib.rs".to_owned(),
        hunk_count: 1,
        old_path: Some("src/lib.rs".to_owned()),
        new_path: Some("src/lib.rs".to_owned()),
        old_content: None,
        new_content: None,
        hunks: vec![FrozenHunk {
            old: Some(0..1),
            new: Some(0..1),
        }],
        diff_hash: String::new(),
    };

    let validated = response.validate("request", &[file]).unwrap();

    assert_eq!(validated.items, vec![item]);
    assert_eq!(validated.rejected_items, 0);
}

#[test]
fn file_targets_are_only_accepted_for_files_without_text_hunks() {
    let item = |path: &str| GuideItem {
        target: GuideTarget::File {
            path: path.to_owned(),
        },
        text: "central idea".to_owned(),
        status: GuideItemStatus::Matched,
    };
    let file = |path: &str, hunk_count| FrozenFile {
        path: path.to_owned(),
        hunk_count,
        old_path: None,
        new_path: None,
        old_content: None,
        new_content: None,
        hunks: Vec::new(),
        diff_hash: String::new(),
    };
    let response = GuideResponse {
        schema_version: 1,
        request_id: "request".to_owned(),
        items: vec![item("binary.bin"), item("source.rs")],
    };

    let validated = response
        .validate("request", &[file("binary.bin", 0), file("source.rs", 1)])
        .unwrap();

    assert_eq!(validated.items, vec![item("binary.bin")]);
    assert_eq!(validated.rejected_items, 1);
}

#[test]
fn separate_line_targets_are_allowed_in_one_new_file_hunk() {
    let line_target = |first_line, last_line, text: &str| GuideItem {
        target: GuideTarget::Lines {
            path: "src/lib.rs".to_owned(),
            old: None,
            new: Some(GuideLineRange {
                first_line,
                last_line,
            }),
        },
        text: text.to_owned(),
        status: GuideItemStatus::Matched,
    };
    let response = GuideResponse {
        schema_version: 1,
        request_id: "request".to_owned(),
        items: vec![
            line_target(1, 10, "first idea"),
            line_target(20, 30, "second idea"),
            line_target(25, 35, "overlap"),
        ],
    };
    let file = FrozenFile {
        path: "src/lib.rs".to_owned(),
        hunk_count: 1,
        old_path: None,
        new_path: Some("src/lib.rs".to_owned()),
        old_content: None,
        new_content: Some(Vec::new()),
        hunks: vec![FrozenHunk {
            old: None,
            new: Some(0..100),
        }],
        diff_hash: "diff".to_owned(),
    };

    let validated = response.validate("request", &[file]).unwrap();

    assert_eq!(validated.items.len(), 2);
    assert_eq!(validated.rejected_items, 1);
}

#[test]
fn line_targets_require_one_based_ordered_ranges() {
    let line_item = |first_line, last_line| GuideItem {
        target: GuideTarget::Lines {
            path: "src/lib.rs".to_owned(),
            old: None,
            new: Some(GuideLineRange {
                first_line,
                last_line,
            }),
        },
        text: "central idea".to_owned(),
        status: GuideItemStatus::Matched,
    };
    let response = GuideResponse {
        schema_version: 1,
        request_id: "request".to_owned(),
        items: vec![line_item(1, 1), line_item(0, 1), line_item(2, 1)],
    };
    let file = FrozenFile {
        path: "src/lib.rs".to_owned(),
        hunk_count: 1,
        old_path: None,
        new_path: Some("src/lib.rs".to_owned()),
        old_content: None,
        new_content: Some(Vec::new()),
        hunks: vec![FrozenHunk {
            old: None,
            new: Some(0..3),
        }],
        diff_hash: String::new(),
    };

    let validated = response.validate("request", &[file]).unwrap();

    assert_eq!(validated.items, vec![line_item(1, 1)]);
    assert_eq!(validated.rejected_items, 2);
}

#[test]
fn adjacent_line_targets_are_accepted_in_either_order() {
    let line_item = |path: &str, first_line, last_line| GuideItem {
        target: GuideTarget::Lines {
            path: path.to_owned(),
            old: None,
            new: Some(GuideLineRange {
                first_line,
                last_line,
            }),
        },
        text: "central idea".to_owned(),
        status: GuideItemStatus::Matched,
    };
    let response = GuideResponse {
        schema_version: 1,
        request_id: "request".to_owned(),
        items: vec![
            line_item("ascending.rs", 1, 10),
            line_item("ascending.rs", 11, 20),
            line_item("descending.rs", 11, 20),
            line_item("descending.rs", 1, 10),
        ],
    };
    let file = |path: &str| FrozenFile {
        path: path.to_owned(),
        hunk_count: 1,
        old_path: None,
        new_path: Some(path.to_owned()),
        old_content: None,
        new_content: Some(Vec::new()),
        hunks: vec![FrozenHunk {
            old: None,
            new: Some(0..20),
        }],
        diff_hash: String::new(),
    };

    let validated = response
        .validate("request", &[file("ascending.rs"), file("descending.rs")])
        .unwrap();

    assert_eq!(validated.items.len(), 4);
    assert_eq!(validated.rejected_items, 0);
}

#[test]
fn old_and_new_line_ranges_must_cover_the_same_hunks() {
    let response = GuideResponse {
        schema_version: 1,
        request_id: "request".to_owned(),
        items: vec![GuideItem {
            target: GuideTarget::Lines {
                path: "src/lib.rs".to_owned(),
                old: Some(GuideLineRange {
                    first_line: 1,
                    last_line: 1,
                }),
                new: Some(GuideLineRange {
                    first_line: 10,
                    last_line: 10,
                }),
            },
            text: "mismatched operations".to_owned(),
            status: GuideItemStatus::Matched,
        }],
    };
    let file = FrozenFile {
        path: "src/lib.rs".to_owned(),
        hunk_count: 2,
        old_path: Some("src/lib.rs".to_owned()),
        new_path: Some("src/lib.rs".to_owned()),
        old_content: None,
        new_content: None,
        hunks: vec![
            FrozenHunk {
                old: Some(0..1),
                new: Some(0..1),
            },
            FrozenHunk {
                old: Some(9..10),
                new: Some(9..10),
            },
        ],
        diff_hash: String::new(),
    };

    let validated = response.validate("request", &[file]).unwrap();

    assert!(validated.items.is_empty());
    assert_eq!(validated.rejected_items, 1);
}

#[test]
fn file_scoped_item_replacement_keeps_items_for_other_files() {
    let item = |path: &str, text: &str| GuideItem {
        target: GuideTarget::Hunks {
            path: path.to_owned(),
            first_hunk: 1,
            last_hunk: 1,
        },
        text: text.to_owned(),
        status: GuideItemStatus::Matched,
    };

    let replaced = replace_items(
        &[item("one.rs", "old one"), item("two.rs", "old two")],
        &GuideScope::File {
            path: "one.rs".to_owned(),
        },
        vec![item("one.rs", "new one")],
    );

    assert_eq!(
        replaced,
        vec![item("one.rs", "new one"), item("two.rs", "old two")]
    );
}

fn assert_added_file_line_target_maps(
    source_content: &[u8],
    source_target: GuideLineRange,
    current_content: &[u8],
    expected_target: GuideLineRange,
) {
    let added_file = |content: &[u8], diff_hash: &str| {
        let line_count = content
            .split_inclusive(|byte| *byte == b'\n')
            .count()
            .try_into()
            .unwrap();
        FrozenFile {
            path: "src/lib.rs".to_owned(),
            hunk_count: 1,
            old_path: None,
            new_path: Some("src/lib.rs".to_owned()),
            old_content: None,
            new_content: Some(content.to_vec()),
            hunks: vec![FrozenHunk {
                old: None,
                new: Some(0..line_count),
            }],
            diff_hash: diff_hash.to_owned(),
        }
    };
    let source = added_file(source_content, "source");
    let item = GuideItem {
        target: GuideTarget::Lines {
            path: source.path.clone(),
            old: None,
            new: Some(source_target),
        },
        text: "central idea".to_owned(),
        status: GuideItemStatus::Matched,
    };
    let [anchored] = anchor_items(&[item], &[source], "checkpoint")
        .try_into()
        .unwrap();
    let current = added_file(current_content, "current");

    let mapped = map_anchored_item(&anchored, &current).unwrap();

    assert_eq!(
        mapped.target,
        GuideTarget::Lines {
            path: "src/lib.rs".to_owned(),
            old: None,
            new: Some(expected_target),
        }
    );
}

#[test]
fn a_line_target_moves_after_an_insertion_before_it() {
    assert_added_file_line_target_maps(
        b"one\ntwo\nthree\nfour\nfive\n",
        GuideLineRange {
            first_line: 3,
            last_line: 4,
        },
        b"zero\none\ntwo\nthree\nfour\nfive\n",
        GuideLineRange {
            first_line: 4,
            last_line: 5,
        },
    );
}

#[test]
fn a_line_target_moves_after_a_deletion_before_it() {
    assert_added_file_line_target_maps(
        b"zero\none\ntwo\nthree\nfour\nfive\n",
        GuideLineRange {
            first_line: 4,
            last_line: 5,
        },
        b"two\nthree\nfour\nfive\n",
        GuideLineRange {
            first_line: 2,
            last_line: 3,
        },
    );
}

#[test]
fn anchored_item_moves_after_lines_are_inserted_before_it() {
    let item = AnchoredGuideItem {
        text: "central idea".to_owned(),
        anchor: DiffRangeAnchor {
            source_checkpoint: "source".to_owned(),
            old_path: Some("src/lib.rs".to_owned()),
            new_path: Some("src/lib.rs".to_owned()),
            old_lines: Some(1..2),
            new_lines: Some(1..2),
            target_kind: GuideAnchorKind::Hunks,
            source_hunk_count: 1,
            old_content: Some(b"a\ntarget\n".to_vec()),
            new_content: Some(b"a\nchanged\n".to_vec()),
            diff_hash: "source".to_owned(),
        },
    };
    let current = FrozenFile {
        path: "src/lib.rs".to_owned(),
        hunk_count: 1,
        old_path: Some("src/lib.rs".to_owned()),
        new_path: Some("src/lib.rs".to_owned()),
        old_content: Some(b"before\na\ntarget\n".to_vec()),
        new_content: Some(b"before\na\nchanged\n".to_vec()),
        hunks: vec![FrozenHunk {
            old: Some(2..3),
            new: Some(2..3),
        }],
        diff_hash: "current".to_owned(),
    };
    let mapped = map_anchored_item(&item, &current).unwrap();
    assert_eq!(mapped.status, GuideItemStatus::Stale);
    assert_eq!(
        mapped.target,
        GuideTarget::Hunks {
            path: "src/lib.rs".to_owned(),
            first_hunk: 1,
            last_hunk: 1,
        }
    );
}

#[test]
fn line_anchor_requires_both_sides_to_map_to_the_same_hunks() {
    let anchored = |old_lines, new_lines| AnchoredGuideItem {
        text: "central idea".to_owned(),
        anchor: DiffRangeAnchor {
            source_checkpoint: "source".to_owned(),
            old_path: Some("src/lib.rs".to_owned()),
            new_path: Some("src/lib.rs".to_owned()),
            old_lines,
            new_lines,
            target_kind: GuideAnchorKind::Lines,
            source_hunk_count: 2,
            old_content: Some(b"a\nb\nc\n".to_vec()),
            new_content: Some(b"a\nb\nc\n".to_vec()),
            diff_hash: "source".to_owned(),
        },
    };
    let current = FrozenFile {
        path: "src/lib.rs".to_owned(),
        hunk_count: 2,
        old_path: Some("src/lib.rs".to_owned()),
        new_path: Some("src/lib.rs".to_owned()),
        old_content: Some(b"a\nb\nc\n".to_vec()),
        new_content: Some(b"a\nb\nc\n".to_vec()),
        hunks: vec![
            FrozenHunk {
                old: Some(0..1),
                new: Some(0..1),
            },
            FrozenHunk {
                old: Some(2..3),
                new: Some(2..3),
            },
        ],
        diff_hash: "current".to_owned(),
    };

    assert!(map_anchored_item(&anchored(Some(0..1), Some(0..1)), &current).is_some());
    assert_eq!(
        map_anchored_item(&anchored(Some(0..1), Some(2..3)), &current),
        None
    );
}

#[test]
fn one_sided_hunk_anchor_maps_to_an_added_hunk() {
    let item = AnchoredGuideItem {
        text: "central idea".to_owned(),
        anchor: DiffRangeAnchor {
            source_checkpoint: "source".to_owned(),
            old_path: None,
            new_path: Some("src/lib.rs".to_owned()),
            old_lines: None,
            new_lines: Some(0..1),
            target_kind: GuideAnchorKind::Hunks,
            source_hunk_count: 1,
            old_content: None,
            new_content: Some(b"new\n".to_vec()),
            diff_hash: "source".to_owned(),
        },
    };
    let current = FrozenFile {
        path: "src/lib.rs".to_owned(),
        hunk_count: 1,
        old_path: None,
        new_path: Some("src/lib.rs".to_owned()),
        old_content: None,
        new_content: Some(b"new\n".to_vec()),
        hunks: vec![FrozenHunk {
            old: None,
            new: Some(0..1),
        }],
        diff_hash: "current".to_owned(),
    };

    assert_eq!(
        map_anchored_item(&item, &current).map(|item| item.target),
        Some(GuideTarget::Hunks {
            path: "src/lib.rs".to_owned(),
            first_hunk: 1,
            last_hunk: 1,
        })
    );
}

#[test]
fn hunk_anchor_maps_consecutive_hunks_after_an_unrelated_hunk() {
    let content = b"zero\none\ntwo\nthree\nfour\n".to_vec();
    let item = AnchoredGuideItem {
        text: "central idea".to_owned(),
        anchor: DiffRangeAnchor {
            source_checkpoint: "source".to_owned(),
            old_path: Some("src/lib.rs".to_owned()),
            new_path: Some("src/lib.rs".to_owned()),
            old_lines: Some(2..5),
            new_lines: Some(2..5),
            target_kind: GuideAnchorKind::Hunks,
            source_hunk_count: 2,
            old_content: Some(content.clone()),
            new_content: Some(content.clone()),
            diff_hash: "source".to_owned(),
        },
    };
    let current = FrozenFile {
        path: "src/lib.rs".to_owned(),
        hunk_count: 3,
        old_path: Some("src/lib.rs".to_owned()),
        new_path: Some("src/lib.rs".to_owned()),
        old_content: Some(content.clone()),
        new_content: Some(content),
        hunks: vec![
            FrozenHunk {
                old: Some(0..1),
                new: Some(0..1),
            },
            FrozenHunk {
                old: Some(2..3),
                new: Some(2..3),
            },
            FrozenHunk {
                old: Some(4..5),
                new: Some(4..5),
            },
        ],
        diff_hash: "current".to_owned(),
    };

    assert_eq!(
        map_anchored_item(&item, &current).map(|item| item.target),
        Some(GuideTarget::Hunks {
            path: "src/lib.rs".to_owned(),
            first_hunk: 2,
            last_hunk: 3,
        })
    );
}

#[test]
fn hunk_anchor_rejects_an_unexplained_line_on_one_side() {
    let item = AnchoredGuideItem {
        text: "central idea".to_owned(),
        anchor: DiffRangeAnchor {
            source_checkpoint: "source".to_owned(),
            old_path: Some("src/lib.rs".to_owned()),
            new_path: Some("src/lib.rs".to_owned()),
            old_lines: Some(0..1),
            new_lines: Some(0..1),
            target_kind: GuideAnchorKind::Hunks,
            source_hunk_count: 1,
            old_content: Some(b"old\n".to_vec()),
            new_content: Some(b"new\n".to_vec()),
            diff_hash: "source".to_owned(),
        },
    };
    let current = FrozenFile {
        path: "src/lib.rs".to_owned(),
        hunk_count: 1,
        old_path: Some("src/lib.rs".to_owned()),
        new_path: Some("src/lib.rs".to_owned()),
        old_content: Some(b"old\nunexplained\n".to_vec()),
        new_content: Some(b"new\n".to_vec()),
        hunks: vec![FrozenHunk {
            old: Some(0..2),
            new: Some(0..1),
        }],
        diff_hash: "current".to_owned(),
    };

    assert_eq!(map_anchored_item(&item, &current), None);
}

#[test]
fn anchors_without_paths_do_not_match_each_other() {
    let item = AnchoredGuideItem {
        text: "notice".to_owned(),
        anchor: DiffRangeAnchor {
            source_checkpoint: "source".to_owned(),
            old_path: None,
            new_path: None,
            old_lines: None,
            new_lines: None,
            target_kind: GuideAnchorKind::Hunks,
            source_hunk_count: 0,
            old_content: None,
            new_content: None,
            diff_hash: String::new(),
        },
    };
    let current = FrozenFile {
        path: "other".to_owned(),
        hunk_count: 0,
        old_path: None,
        new_path: None,
        old_content: None,
        new_content: None,
        hunks: Vec::new(),
        diff_hash: String::new(),
    };

    assert_eq!(map_anchored_item(&item, &current), None);
}

#[test]
fn file_scoped_replacement_keeps_other_path_anchors() {
    let anchored = |path: &str| AnchoredGuideItem {
        text: path.to_owned(),
        anchor: DiffRangeAnchor {
            source_checkpoint: "source".to_owned(),
            old_path: Some(path.to_owned()),
            new_path: Some(path.to_owned()),
            old_lines: None,
            new_lines: None,
            target_kind: GuideAnchorKind::Hunks,
            source_hunk_count: 0,
            old_content: None,
            new_content: None,
            diff_hash: path.to_owned(),
        },
    };

    let result = replace_anchored_items(
        &[anchored("one"), anchored("two")],
        &GuideScope::File {
            path: "one".to_owned(),
        },
        vec![anchored("one")],
        &[zero_hunk_file("one", "one"), zero_hunk_file("two", "two")],
    );

    assert_eq!(result.len(), 2);
    assert!(result.iter().any(|item| item.text == "two"));
}

#[test]
fn file_scoped_replacement_removes_an_anchor_mapped_through_a_rename() {
    let previous = AnchoredGuideItem {
        text: "old explanation".to_owned(),
        anchor: DiffRangeAnchor {
            source_checkpoint: "source".to_owned(),
            old_path: Some("old.rs".to_owned()),
            new_path: Some("old.rs".to_owned()),
            old_lines: None,
            new_lines: None,
            target_kind: GuideAnchorKind::Hunks,
            source_hunk_count: 0,
            old_content: None,
            new_content: None,
            diff_hash: "rename".to_owned(),
        },
    };
    let renamed = FrozenFile {
        path: "new.rs".to_owned(),
        hunk_count: 0,
        old_path: Some("old.rs".to_owned()),
        new_path: Some("new.rs".to_owned()),
        old_content: None,
        new_content: None,
        hunks: Vec::new(),
        diff_hash: "rename".to_owned(),
    };

    let result = replace_anchored_items(
        &[previous],
        &GuideScope::File {
            path: "new.rs".to_owned(),
        },
        Vec::new(),
        &[renamed],
    );

    assert!(result.is_empty());
}

#[test]
fn file_scoped_replacement_removes_an_anchor_with_an_overlapping_edit() {
    let previous = AnchoredGuideItem {
        text: "outdated explanation".to_owned(),
        anchor: DiffRangeAnchor {
            source_checkpoint: "source".to_owned(),
            old_path: Some("src/lib.rs".to_owned()),
            new_path: Some("src/lib.rs".to_owned()),
            old_lines: Some(0..1),
            new_lines: Some(0..1),
            target_kind: GuideAnchorKind::Hunks,
            source_hunk_count: 1,
            old_content: Some(b"old\n".to_vec()),
            new_content: Some(b"new\n".to_vec()),
            diff_hash: "source".to_owned(),
        },
    };
    let edited = FrozenFile {
        path: "src/lib.rs".to_owned(),
        hunk_count: 1,
        old_path: Some("src/lib.rs".to_owned()),
        new_path: Some("src/lib.rs".to_owned()),
        old_content: Some(b"changed old\n".to_vec()),
        new_content: Some(b"changed new\n".to_vec()),
        hunks: vec![FrozenHunk {
            old: Some(0..1),
            new: Some(0..1),
        }],
        diff_hash: "current".to_owned(),
    };
    assert_eq!(map_anchored_item(&previous, &edited), None);

    let result = replace_anchored_items(
        &[previous],
        &GuideScope::File {
            path: "src/lib.rs".to_owned(),
        },
        Vec::new(),
        &[edited],
    );

    assert!(result.is_empty());
}

fn zero_hunk_file(path: &str, diff_hash: &str) -> FrozenFile {
    FrozenFile {
        path: path.to_owned(),
        hunk_count: 0,
        old_path: Some(path.to_owned()),
        new_path: Some(path.to_owned()),
        old_content: None,
        new_content: None,
        hunks: Vec::new(),
        diff_hash: diff_hash.to_owned(),
    }
}

#[test]
fn changed_zero_hunk_file_does_not_keep_its_guide() {
    let item = AnchoredGuideItem {
        text: "binary format".to_owned(),
        anchor: DiffRangeAnchor {
            source_checkpoint: "source".to_owned(),
            old_path: Some("image.png".to_owned()),
            new_path: Some("image.png".to_owned()),
            old_lines: None,
            new_lines: None,
            target_kind: GuideAnchorKind::Hunks,
            source_hunk_count: 0,
            old_content: None,
            new_content: None,
            diff_hash: "old diff".to_owned(),
        },
    };
    let current = FrozenFile {
        path: "image.png".to_owned(),
        hunk_count: 0,
        old_path: Some("image.png".to_owned()),
        new_path: Some("image.png".to_owned()),
        old_content: None,
        new_content: None,
        hunks: Vec::new(),
        diff_hash: "new diff".to_owned(),
    };

    assert_eq!(map_anchored_item(&item, &current), None);
}

#[test]
fn unchanged_zero_hunk_file_keeps_its_guide() {
    let item = AnchoredGuideItem {
        text: "binary format".to_owned(),
        anchor: DiffRangeAnchor {
            source_checkpoint: "source".to_owned(),
            old_path: Some("image.png".to_owned()),
            new_path: Some("image.png".to_owned()),
            old_lines: None,
            new_lines: None,
            target_kind: GuideAnchorKind::Hunks,
            source_hunk_count: 0,
            old_content: None,
            new_content: None,
            diff_hash: "same diff".to_owned(),
        },
    };

    assert_eq!(
        map_anchored_item(&item, &zero_hunk_file("image.png", "same diff")).map(|item| item.target),
        Some(GuideTarget::File {
            path: "image.png".to_owned(),
        })
    );
}

fn anchored_new_line_item(path: &str, line: u32, text: &str) -> AnchoredGuideItem {
    AnchoredGuideItem {
        text: text.to_owned(),
        anchor: DiffRangeAnchor {
            source_checkpoint: "source".to_owned(),
            old_path: None,
            new_path: Some(path.to_owned()),
            old_lines: None,
            new_lines: Some(line..line.saturating_add(1)),
            target_kind: GuideAnchorKind::Lines,
            source_hunk_count: 1,
            old_content: None,
            new_content: Some(b"zero\none\ntwo\nthree\n".to_vec()),
            diff_hash: "source".to_owned(),
        },
    }
}

fn added_text_file(path: &str) -> FrozenFile {
    FrozenFile {
        path: path.to_owned(),
        hunk_count: 1,
        old_path: None,
        new_path: Some(path.to_owned()),
        old_content: None,
        new_content: Some(b"zero\none\ntwo\nthree\n".to_vec()),
        hunks: vec![FrozenHunk {
            old: None,
            new: Some(0..4),
        }],
        diff_hash: "current".to_owned(),
    }
}

#[test]
fn ordered_anchored_items_map_to_the_current_file() {
    let items = vec![
        anchored_new_line_item("src/lib.rs", 0, "first"),
        anchored_new_line_item("src/lib.rs", 2, "second"),
    ];

    let mapped = map_anchored_items(&items, &[added_text_file("src/lib.rs")]);

    assert_eq!(mapped.len(), 2);
    assert_eq!(mapped[0].text, "first");
    assert_eq!(mapped[1].text, "second");
}

#[test]
fn reordered_or_overlapping_anchored_items_are_omitted() {
    let items = vec![
        anchored_new_line_item("src/lib.rs", 2, "later"),
        anchored_new_line_item("src/lib.rs", 1, "earlier"),
        anchored_new_line_item("src/lib.rs", 2, "overlap"),
    ];

    let mapped = map_anchored_items(&items, &[added_text_file("src/lib.rs")]);

    assert_eq!(mapped.len(), 1);
    assert_eq!(mapped[0].text, "later");
}

#[test]
fn an_anchor_that_matches_multiple_current_files_is_omitted() {
    let item = anchored_new_line_item("src/lib.rs", 1, "ambiguous");
    let first = added_text_file("src/lib.rs");
    let mut second = added_text_file("copy.rs");
    second.new_path = Some("src/lib.rs".to_owned());

    assert!(map_anchored_items(&[item], &[first, second]).is_empty());
}

fn changed_text_file(path: &str) -> FrozenFile {
    FrozenFile {
        path: path.to_owned(),
        hunk_count: 2,
        old_path: Some(path.to_owned()),
        new_path: Some(path.to_owned()),
        old_content: Some(b"old zero\nsame\nold two\n".to_vec()),
        new_content: Some(b"new zero\nsame\nnew two\n".to_vec()),
        hunks: vec![
            FrozenHunk {
                old: Some(0..1),
                new: Some(0..1),
            },
            FrozenHunk {
                old: Some(2..3),
                new: Some(2..3),
            },
        ],
        diff_hash: "diff".to_owned(),
    }
}

fn hunk_item(path: &str, hunk: usize, text: &str) -> GuideItem {
    GuideItem {
        target: GuideTarget::Hunks {
            path: path.to_owned(),
            first_hunk: hunk,
            last_hunk: hunk,
        },
        text: text.to_owned(),
        status: GuideItemStatus::Matched,
    }
}

#[test]
fn anchoring_a_later_hunk_uses_its_exact_range() {
    let file = changed_text_file("src/lib.rs");

    let anchored = anchor_items(
        &[hunk_item("src/lib.rs", 2, "second operation")],
        &[file],
        "checkpoint",
    );

    assert_eq!(anchored.len(), 1);
    assert_eq!(anchored[0].anchor.old_lines, Some(2..3));
    assert_eq!(anchored[0].anchor.new_lines, Some(2..3));
}

#[test]
fn adjacent_hunk_items_and_later_files_keep_their_order() {
    let first_file = changed_text_file("one.rs");
    let second_file = changed_text_file("two.rs");
    let items = vec![
        hunk_item("one.rs", 1, "first hunk"),
        hunk_item("one.rs", 2, "adjacent hunk"),
        hunk_item("two.rs", 1, "later file"),
    ];
    let anchors = anchor_items(
        &items,
        &[first_file.clone(), second_file.clone()],
        "checkpoint",
    );

    let mapped = map_anchored_items(&anchors, &[first_file, second_file]);

    assert_eq!(mapped.len(), 3);
    assert_eq!(mapped[0].text, "first hunk");
    assert_eq!(mapped[1].text, "adjacent hunk");
    assert_eq!(mapped[2].text, "later file");
}

#[test]
fn a_hunk_item_cannot_follow_a_line_item_in_the_same_hunk() {
    let file = changed_text_file("src/lib.rs");
    let items = vec![
        GuideItem {
            target: GuideTarget::Lines {
                path: "src/lib.rs".to_owned(),
                old: Some(GuideLineRange {
                    first_line: 3,
                    last_line: 3,
                }),
                new: Some(GuideLineRange {
                    first_line: 3,
                    last_line: 3,
                }),
            },
            text: "part of the second hunk".to_owned(),
            status: GuideItemStatus::Matched,
        },
        hunk_item("src/lib.rs", 2, "the complete second hunk"),
    ];
    let anchors = anchor_items(&items, std::slice::from_ref(&file), "checkpoint");

    let mapped = map_anchored_items(&anchors, &[file]);

    assert_eq!(mapped.len(), 1);
    assert_eq!(mapped[0].text, "part of the second hunk");
}
