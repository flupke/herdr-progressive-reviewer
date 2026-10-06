use super::*;

#[test]
fn the_glyphs_tui_test_could_not_render_are_read_from_its_error() {
    let error = "recording rasterizer could not render glyphs: '🦀' (U+1F980), '☃' (U+2603); \
                 install an outline font containing them or set TUI_TEST_RECORDING_FONT_FAMILIES";
    assert_eq!(missing_glyphs(error), vec!['🦀', '☃']);
    assert!(missing_glyphs("another error").is_empty());
}

#[test]
fn a_missing_glyph_keeps_its_cells() {
    assert_eq!(
        replace("\x1b[1mUnicode: 🦀, ☃.", &['🦀', '☃']),
        "\x1b[1mUnicode: ??, ?."
    );
}
