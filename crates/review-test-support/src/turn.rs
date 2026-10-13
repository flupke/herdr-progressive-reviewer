//! An Explore prompt as an agent reads it: the prompt names its turn's index, and the agent
//! reads the turn's files beside it.

use std::path::Path;

/// The names `review-explore-runner` gives a turn's index, the round's data and the turn's: that
/// crate stays out of this one, which the pane's e2e tests build too.
const INDEX: &str = "index.md";
const DATA: [&str; 2] = ["round.md", "turn.md"];

/// `prompt` followed by the round's data and the turn's, `round.md` then `turn.md`, when it
/// names a turn's index: the change, identity, unreviewed lines and answer that a test reads
/// from what the agent got. Any other prompt, and one whose turn's files are gone, comes back as
/// it is. Call it when the prompt arrives: a turn's files live until the next prompt.
pub fn with_turn(prompt: &str) -> String {
    let Some(directory) = prompt
        .split_whitespace()
        .find_map(|word| word.strip_suffix(INDEX)?.strip_suffix('/'))
    else {
        return prompt.to_owned();
    };
    let data: Option<Vec<_>> = DATA
        .iter()
        .map(|name| std::fs::read_to_string(Path::new(directory).join(name)).ok())
        .collect();
    match data {
        Some(data) => format!("{prompt}\n{}", data.join("\n")),
        None => prompt.to_owned(),
    }
}
