use progressive_reviewer_core::version::{check_runtime_versions, herdr_program_from_environment};

#[test]
#[ignore = "requires installed Herdr and jj binaries"]
fn installed_tools_meet_the_minimum_versions() {
    check_runtime_versions(herdr_program_from_environment(), "jj").unwrap();
}
