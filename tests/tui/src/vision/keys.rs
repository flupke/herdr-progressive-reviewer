//! Key names as a vision session takes them: Herdr's names, which Herdr encodes for the
//! reviewer's keyboard mode, and the navigation keys Herdr does not name, encoded here.

/// How one named key reaches the reviewer.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum KeyInput {
    /// A name Herdr encodes, or refuses with an error of its own.
    Herdr(String),
    /// The bytes of a key Herdr does not name.
    Bytes(String),
}

/// The navigation keys Herdr 0.9 does not name, with the final parameter or letter of their
/// CSI sequence. The reviewer's keyboard mode (disambiguated escape codes) reads them in their
/// legacy form.
const NAVIGATION: &[(&str, &str)] = &[
    ("pageup", "5~"),
    ("pagedown", "6~"),
    ("home", "H"),
    ("end", "F"),
    ("insert", "2~"),
    ("delete", "3~"),
];

impl KeyInput {
    pub(crate) fn parse(name: &str) -> Self {
        let lower = name.to_ascii_lowercase().replace(['_', '-'], "");
        let (modifiers, key) = match lower.rsplit_once('+') {
            Some((modifiers, key)) if !key.is_empty() => (Some(modifiers), key),
            _ => (None, lower.as_str()),
        };
        let Some((_, last)) = NAVIGATION.iter().find(|(named, _)| *named == key) else {
            return Self::Herdr(name.to_owned());
        };
        let Some(code) = modifiers.map_or(Some(1), modifier_code) else {
            return Self::Herdr(name.to_owned());
        };
        let (number, end) = match last.strip_suffix('~') {
            Some(number) => (number, "~"),
            None => ("1", *last),
        };
        Self::Bytes(match (code, end) {
            (1, "~") => format!("\x1b[{number}~"),
            (1, letter) => format!("\x1b[{letter}"),
            (code, end) => format!("\x1b[{number};{code}{end}"),
        })
    }
}

/// The xterm modifier parameter of `modifiers` (`ctrl+shift`), or `None` for an unknown one.
fn modifier_code(modifiers: &str) -> Option<u8> {
    let mut code = 0;
    for modifier in modifiers.split('+') {
        code |= match modifier {
            "shift" => 1,
            "alt" | "meta" => 2,
            "ctrl" | "control" => 4,
            _ => return None,
        };
    }
    Some(code + 1)
}

#[cfg(test)]
#[path = "keys.tests.rs"]
mod tests;
