use ratatui::style::Color;
use two_face::theme::EmbeddedThemeName;

use super::SyntaxHighlighter;

#[test]
fn module_and_jsx_files_are_colored_as_their_language() {
    let highlighter = SyntaxHighlighter::new(EmbeddedThemeName::CatppuccinMocha, Color::White);
    let code = "import { spawn } from \"node:child_process\";\nconst view = <App name=\"x\" />;\n";
    let colors = |path: &str| {
        highlighter
            .highlight_snippet(path, code)
            .concat()
            .into_iter()
            .map(|token| token.color)
            .collect::<Vec<_>>()
    };
    for colored in ["start-web.js", "view.tsx"] {
        assert!(
            colors(colored).iter().any(|color| *color != Color::White),
            "{colored} is colored"
        );
    }
    for (path, read_as) in [
        ("start-web.mjs", "start-web.js"),
        ("start-web.cjs", "start-web.js"),
        ("view.jsx", "view.tsx"),
        ("START-WEB.MJS", "start-web.js"),
        // The languages of Markdown code blocks.
        ("mjs", "js"),
        ("jsx", "tsx"),
    ] {
        assert_eq!(colors(path), colors(read_as), "{path}");
    }
}
