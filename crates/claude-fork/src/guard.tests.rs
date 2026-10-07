use super::*;

fn call(tool: &str, command: &str) -> Option<String> {
    refusal(&serde_json::json!({"tool_name": tool, "tool_input": {"command": command}}))
}

#[test]
fn a_fork_reads_and_submits_and_writes_nothing() {
    for allowed in [
        "Read",
        "Grep",
        "Glob",
        "Agent",
        "mcp__herdr_reviewer__submit_question",
    ] {
        assert!(call(allowed, "").is_none(), "{allowed}");
    }
    for refused in [
        "Edit",
        "Write",
        "NotebookEdit",
        "mcp__herdr_reviewer__reply",
    ] {
        assert!(call(refused, "").is_some(), "{refused}");
    }
}

#[test]
fn a_fork_runs_reading_commands_only() {
    for allowed in [
        "jj --ignore-working-copy diff -r @ --git | head -50",
        "git log --oneline -5",
        "git --no-pager show HEAD~1:src/lib.rs",
        "find . -O3 -name x",
        "git ls-files --others --exclude-standard",
        "git log -p -- src",
        "git diff --text",
        "git log --filter=blob:none",
        "grep -R x src",
        "rg -n policy src | cut -c1-80",
        "find . -name '*.rs'",
        "ls src/*.rs",
        "cat ~/notes",
        "find . ! -name x",
        "rg -n x src 2>/dev/null | head 2>&1",
    ] {
        assert_eq!(call("Bash", allowed), None, "{allowed}");
    }
    for refused in [
        "jj --ignore-working-copy new",
        "git commit -m x",
        "rm -rf x",
        "cat a > b",
        "ls; rm x",
        "ls && rm x",
        "echo $(rm x)",
        "find . -delete",
        "find . -exec rm x +",
        "cat x\nrm -rf y",
        "rg --pre ./script x",
        "rg --pre=./script x",
        "rg --hostname-bin=./script x",
        "git diff --output=patch",
        "git diff --out=patch",
        "git diff --ext",
        "git grep -O x",
        "git grep -Ovim x",
        "git grep -nOvim x",
        "git grep -3Ovim x",
        "git grep -e x -1O",
        "git grep --open-files x",
        "git -c core.pager=sh log",
        "git -c log commit -m x",
        "git -C log commit -m x",
        "git -C ../other log",
        "git --exec-path=. log",
        "tree -o out",
        "sort -o out x",
        "jj --ignore-working-copy diff --config ui.diff-formatter=sh",
        "jj --ignore-working-copy diff --config=ui.pager=sh",
        "jj --ignore-working-copy diff --tool=x",
        "jj --ignore-working-copy -R log new",
        "jj -R ../other --ignore-working-copy log",
        "jj --ignore-working-copy log -R ../other",
        "jj --ignore-working-copy log -pR../other",
        "jj --ignore-working-copy diff --repository=../other",
        "jj --ignore-working-copy --at-op log new",
        "jj --ignore-working-copy file annotate x",
        "jj --ignore-working-copy --debug log",
        "./jj --ignore-working-copy log",
        "cargo test",
        "",
    ] {
        assert!(call("Bash", refused).is_some(), "{refused}");
    }
}

#[test]
fn quotes_hide_shell_forms_and_unquoted_ones_still_count() {
    for allowed in [
        "rg '&self'",
        "rg '-> Result'",
        "rg -e '-> Option'",
        "rg 'foo$'",
        "rg \"foo$\"",
        "grep -e 'a\\|b' src",
        "rg 'a;b' | head -3",
        "rg 'x > y' src",
        "rg '$(rm x)' src",
        "rg '`rm x`' src",
        "rg \"\\$(rm x)\" src",
        "rg 'a|b' | rg \"c|d\" | head",
        r"rg a\;b",
    ] {
        assert_eq!(call("Bash", allowed), None, "{allowed}");
    }
    for refused in [
        "rg 'x' ; rm y",
        "rg 'x'&& rm y",
        "rg 'x'>out",
        "rg \"$(rm x)\"",
        "rg \"`rm x`\"",
        "rg 'x' `rm y`",
        "rg 'a\nb'",
        "rg 'unclosed",
        // A quote or a backslash does not hide an option from the guard.
        "rg '--pre'=sh x",
        "rg --p're=sh' x",
        "rg \\-\\-pre=sh x",
        "find . '-delete'",
        "rg \"--pre\" sh x",
        "'rm' x",
        // Words that only a pipe or a list outside quotes would make commands.
        "rg x | rm y",
        "rg x | sh",
        "rg x | xargs rm",
    ] {
        assert!(call("Bash", refused).is_some(), "{refused}");
    }
}

#[test]
fn a_shell_form_refusal_names_the_form() {
    let reason = call("Bash", "rg x; rm y").unwrap();
    assert!(reason.contains("`;` outside quotes"), "{reason}");
}

#[test]
fn jj_reads_without_snapshotting_the_working_copy() {
    for allowed in [
        "jj --ignore-working-copy file show -r X -- path",
        "jj --ignore-working-copy file show -r 'abc-' -- src/a.rs",
        "jj --ignore-working-copy file list -r X",
        "jj --ignore-working-copy log -r 'trunk()..@'",
        "jj log --ignore-working-copy -r @-",
        "jj --no-pager --color=never --ignore-working-copy st",
    ] {
        assert_eq!(call("Bash", allowed), None, "{allowed}");
    }
    for snapshots in [
        "jj log",
        "jj diff -r @ --git",
        "jj st",
        "jj file show -r X -- path",
        "jj file list | head",
        // After `--`, it is a path, not an option.
        "jj file show -r X -- --ignore-working-copy",
    ] {
        let reason = call("Bash", snapshots).unwrap_or_else(|| panic!("{snapshots}"));
        assert!(
            reason.contains("--ignore-working-copy"),
            "{snapshots}: {reason}"
        );
    }
    let reason = call("Bash", "jj new").unwrap();
    assert!(!reason.contains("--ignore-working-copy"), "{reason}");
}

#[test]
fn sed_prints_lines_and_does_nothing_else() {
    for allowed in [
        "sed -n 20,30p src/lib.rs",
        "sed -n '20,30p' src/lib.rs",
        "sed -n -e 1p -e '$p' a",
        "sed -ne '/fn main/,/^}/p' a",
        "sed -n '1p; 5,+2p;/x\\/y/p' a",
        "sed -nE '0~4p' a",
        "sed --quiet '$p' a",
        "sed -n --expression 1p --regexp-extended a",
        "sed '10,20p' -n a",
        "sed -n '/^const X/,/^];/p;$p;' a",
        "jj --ignore-working-copy file show -r X -- a | sed -n 20,30p",
    ] {
        assert_eq!(call("Bash", allowed), None, "{allowed}");
    }
    for refused in [
        "sed -i 's/a/b/' x",
        "sed -n -i 1p x",
        "sed -ni 1p x",
        "sed -n 1p -i x",
        "sed --in-place -n 1p x",
        "sed --in -n 1p x",
        "sed -n -f script x",
        "sed 20,30p x",
        "sed -n 1w out x",
        "sed -n '1W out' x",
        "sed -n '1p;w out' x",
        "sed -n '/x/w out' x",
        "sed -n '1r /etc/passwd' x",
        "sed -n '1e rm x' x",
        "sed -n 's/a/b/w out' x",
        "sed -n 's/a/b/e' x",
        "sed -n 's/a/b/p' x",
        "sed -n '/a/p;w out' x",
        "sed -n '1p;;w out' x",
        "sed -n '/[/]/w out' x",
        "sed -n '/a/,/b/w out' x",
        "sed -n '/a\\/w out' x",
        "sed -n 1p\\* x",
        "sed -n 1~2p* x",
        "sed -n -e 1p -e 'w out' x",
        "sed -n x",
        "sed -n 1pw\\ out x",
        "sed -n '$pe date' x",
        "sed -n 1,p x",
        "sed -n ,5p x",
        "sed -n --expression=1p x",
        "sed -n",
    ] {
        assert!(call("Bash", refused).is_some(), "{refused}");
    }
}
