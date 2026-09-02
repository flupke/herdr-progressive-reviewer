use super::*;

#[test]
fn groups_paths_by_baseline_without_duplicates() {
    let first = RepoPath::from_bytes(b"src/first.rs".to_vec());
    let second = RepoPath::from_bytes(b"src/second.rs".to_vec());
    let mut plan = BaselineComparisonPlan::default();

    plan.add(SnapshotId::from("baseline-a".to_owned()), first.clone());
    plan.add(SnapshotId::from("baseline-a".to_owned()), second.clone());
    plan.add(SnapshotId::from("baseline-a".to_owned()), first.clone());
    plan.add(SnapshotId::from("baseline-b".to_owned()), first.clone());

    let groups = plan
        .baselines()
        .map(|(baseline, paths)| (baseline.as_str().to_owned(), paths.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        groups,
        vec![
            (
                "baseline-a".to_owned(),
                BTreeSet::from([first.clone(), second]),
            ),
            ("baseline-b".to_owned(), BTreeSet::from([first])),
        ]
    );
}
