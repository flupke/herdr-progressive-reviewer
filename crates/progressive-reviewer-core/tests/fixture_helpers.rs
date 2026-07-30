#[path = "support/jj.rs"]
mod support;

use support::{JjFixture, JjLayout};

#[test]
fn creates_a_non_colocated_jj_repository() {
    let fixture = JjFixture::new(JjLayout::NonColocated);

    assert!(fixture.root().join(".jj").is_dir());
    assert!(!fixture.root().join(".git").exists());
    assert_eq!(fixture.jj_root(), fixture.root());
}

#[test]
fn creates_a_colocated_jj_repository() {
    let fixture = JjFixture::new(JjLayout::Colocated);

    assert!(fixture.root().join(".jj").is_dir());
    assert!(fixture.root().join(".git").is_dir());
    assert_eq!(fixture.jj_root(), fixture.root());
}

#[test]
fn writes_repository_fixture_files() {
    let fixture = JjFixture::new(JjLayout::NonColocated);

    fixture.write("src/example.txt", b"fixture content\n");

    assert_eq!(
        std::fs::read(fixture.root().join("src/example.txt")).unwrap(),
        b"fixture content\n"
    );
}
