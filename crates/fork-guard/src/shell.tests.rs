use super::*;

/// The texts of the words of each command of the pipe `command`.
fn texts(command: &str) -> Vec<Vec<String>> {
    split_pipe(command)
        .unwrap_or_else(|error| panic!("{command}: {error:?}"))
        .into_iter()
        .map(|words| words.into_iter().map(|word| word.text).collect())
        .collect()
}

#[test]
fn a_pipe_outside_quotes_splits_the_commands() {
    assert_eq!(
        texts("jj diff|head -5 | rg '|' \"|\" a\\|b"),
        [
            vec!["jj", "diff"],
            vec!["head", "-5"],
            vec!["rg", "|", "|", "a|b"]
        ]
    );
}

#[test]
fn error_messages_may_be_discarded_or_mixed_into_the_output() {
    assert_eq!(
        texts("rg x 2>/dev/null | head 2>&1|wc -l 2>&1"),
        [vec!["rg", "x"], vec!["head"], vec!["wc", "-l"]]
    );
}

#[test]
fn shell_forms_outside_quotes_are_refused() {
    for (command, form) in [
        ("ls; rm x", ';'),
        ("ls && rm x", '&'),
        ("ls & rm x", '&'),
        ("ls |& cat", '&'),
        ("cat a > b", '>'),
        ("rg x 2>out", '>'),
        ("rg x 2>/dev/nullx", '>'),
        ("rg x 2>>/dev/null", '>'),
        ("rg x 2> /dev/null", '>'),
        ("rg x x2>/dev/null", '>'),
        ("rg x 2>&1>out", '>'),
        ("rg x 2>&2", '>'),
        ("rg x '2'>/dev/null", '>'),
        ("rg x \\2>/dev/null", '>'),
        ("cat a>>b", '>'),
        ("rg x 2>/dev/null;rm y", '>'),
        ("cat < a", '<'),
        ("cat <(rm x)", '<'),
        ("echo $(rm x)", '$'),
        ("cat $HOME", '$'),
        ("cat $'\\x41'", '$'),
        ("echo `rm x`", '`'),
        ("(rm x)", '('),
        ("rg {--pre=sh,x}", '{'),
        ("cat a'b'}", '}'),
        // Inside double quotes, `$` and the backtick still substitute.
        ("rg \"$(rm x)\"", '$'),
        ("rg \"${x:=y}\"", '$'),
        ("rg \"`rm x`\"", '`'),
    ] {
        assert_eq!(
            split_pipe(command).err(),
            Some(ShellError::Form(form)),
            "{command}"
        );
    }
}

#[test]
fn a_glob_is_refused_where_file_names_could_become_options() {
    for (command, glob) in [
        ("cat *", '*'),
        ("rg x ?", '?'),
        ("find . -name *.rs", '*'),
        ("rg --pre*", '*'),
        ("rg -O*", '*'),
        ("ls ''*", '*'),
        ("ls '-'[a]", '['),
        ("ls ^x", '^'),
        ("ls #x", '#'),
        ("cat ~[name]", '['),
        ("ls ~/src/*.rs", '*'),
    ] {
        assert_eq!(
            split_pipe(command).err(),
            Some(ShellError::LeadingGlob(glob)),
            "{command}"
        );
    }
    let words = split_pipe("ls src/*.rs '*' ~/x HEAD~1 \\*").unwrap();
    let globs: Vec<_> = words[0]
        .iter()
        .map(|word| (word.text.as_str(), word.globs))
        .collect();
    assert_eq!(
        globs,
        [
            ("ls", false),
            ("src/*.rs", true),
            ("*", false),
            ("~/x", false),
            ("HEAD~1", true),
            ("*", false),
        ]
    );
}

#[test]
fn newlines_and_unclosed_quotes_are_refused() {
    for (command, error) in [
        ("cat x\nrm -rf y", ShellError::Control),
        ("rg 'a\nb'", ShellError::Control),
        ("rg \"a\nb\"", ShellError::Control),
        ("rg a\\\nb", ShellError::Control),
        ("cat x\r", ShellError::Control),
        ("rg 'a\tb'", ShellError::Control),
        ("rg x\u{0}", ShellError::Control),
        ("rg 'x", ShellError::Unclosed),
        ("rg \"x", ShellError::Unclosed),
        ("rg \"x\\\"", ShellError::Unclosed),
        ("rg x\\", ShellError::Unclosed),
        ("", ShellError::Empty),
        ("  ", ShellError::Empty),
        ("| cat", ShellError::Empty),
        ("ls |", ShellError::Empty),
        ("ls || rm x", ShellError::Empty),
    ] {
        assert_eq!(split_pipe(command).err(), Some(error), "{command:?}");
    }
}

/// Bash, with pathname expansion off, passes the words the guard reads.
#[test]
fn the_words_are_those_bash_passes() {
    let command = concat!(
        r#"rg '&self' "a b"c\ d \-\-pre '' -e 'a\|b' "x\$y\"z\\w\n" "end$" "#,
        r#"a'b'"c" \| '"' "'" \' \" ~/x HEAD~1 src/*.rs '*' "#,
        r#"--p're=sh' 'it''s' -nO "é" ! = %"#
    );
    let words = split_pipe(command).unwrap();
    let output = std::process::Command::new("bash")
        .args(["-f", "-c", &format!("printf '%s\\0' {command}")])
        .output()
        .unwrap();
    let home = std::env::var("HOME").unwrap();
    let from_bash: Vec<String> = String::from_utf8(output.stdout)
        .unwrap()
        .split_terminator('\0')
        .map(|word| word.replacen(&home, "~", 1))
        .collect();
    let expected: Vec<&str> = words[0].iter().map(|word| word.text.as_str()).collect();
    assert_eq!(from_bash, expected);
}
