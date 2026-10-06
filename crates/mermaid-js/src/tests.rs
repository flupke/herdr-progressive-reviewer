use super::*;

#[test]
fn the_script_is_the_pinned_version() {
    let script = std::str::from_utf8(script()).unwrap();
    assert!(script.contains(&format!(r#"version:"{VERSION}""#)));
}

#[test]
fn the_script_is_served_gzipped() {
    assert!(GZIPPED.len() * 3 < script().len(), "{}", GZIPPED.len());
}
