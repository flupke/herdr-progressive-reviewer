"""Freeze historical file diffs and apply the reviewer's syntax-based policy.

Requires tree-sitter==0.25.2 and tree-sitter-rust==0.24.0. No Jev calls.
Labels are proposed annotations for audit, never model-generated answers.
"""

import argparse
import hashlib
import json
import re
import subprocess
from collections import Counter
from dataclasses import dataclass
from pathlib import Path

from tree_sitter import Language, Parser
import tree_sitter_rust

ROOT = Path(__file__).resolve().parents[5]
PARSER = Parser(Language(tree_sitter_rust.language()))


class History:
    @staticmethod
    def git(*args):
        return subprocess.check_output(["git", *args], cwd=ROOT)

    @classmethod
    def source(cls, commit, path):
        if not cls.git("ls-tree", commit, "--", path):
            return b""  # The path is absent, as with an addition or deletion.
        return cls.git("show", f"{commit}:{path}")

    @classmethod
    def test_proof(cls, commit, path):
        p = Path(path)
        if len(p.parts) > 3 and p.parts[2] == "tests":
            return "Cargo integration-test directory"
        if p.name != "tests.rs" and not p.name.endswith(".tests.rs"):
            return ""
        # A test-like filename alone is insufficient: inspect its module owner.
        for owner in (p.parent / "lib.rs", p.parent / "mod.rs", p.parent / "main.rs",
                      p.with_name(p.name.replace(".tests.rs", ".rs"))):
            source = cls.source(commit, str(owner)).decode("utf-8", errors="replace")
            for match in re.finditer(r'#\[cfg\(test\)\]\s*(?:#\[path\s*=\s*"([^"\n]+)"\]\s*)?mod tests\s*;', source):
                target = owner.parent / (match.group(1) or "tests.rs")
                if target == p:
                    return f"{owner}: #[cfg(test)] module owner"
        return ""


@dataclass
class SourceLabels:
    source: bytes
    categories: dict
    leaves: list
    parse_error: bool

    @staticmethod
    def test_attribute(text):
        if re.fullmatch(rb'#\[(?:cfg\(test\)|test|tokio::test(?:\([^]]*\))?)\]', text):
            return True
        match = re.fullmatch(rb'#\[cfg\(all\((.*)\)\)\]', text, re.S)
        if not match:
            return False
        # A top-level `test` conjunct proves this item cannot exist without tests.
        # Nested `any(test, feature)` does not establish that implication.
        root = PARSER.parse(text + b"\nfn annotation_probe() {}\n").root_node
        attribute = root.named_children[0].named_children[0]
        cfg_tree = attribute.named_children[1]
        arguments = cfg_tree.named_children[1]
        group = []
        for node in arguments.children[1:]:
            if node.type in (",", ")"):
                if len(group) == 1 and group[0].type == "identifier" and group[0].text == b"test":
                    return True
                group = []
            else:
                group.append(node)
        return False

    @classmethod
    def rust(cls, source, test_only=False):
        root = PARSER.parse(source).root_node
        spans = []
        leaves = []
        literal_lines = set()

        def walk(node):
            if node.type in ("string_literal", "raw_string_literal"):
                literal_lines.update(range(node.start_point.row + 1, node.end_point.row + 2))
            if node.type in ("line_comment", "block_comment"):
                spans.append((node.start_byte, node.end_byte, "comment"))
                return
            if node.type == "use_declaration":
                spans.append((node.start_byte, node.end_byte, "import"))
            if node.type == "mod_item":
                body = node.child_by_field_name("body")
                end = body.start_byte + 1 if body else node.end_byte
                spans.append((node.start_byte, end, "module"))
                if body:
                    spans.append((body.end_byte - 1, body.end_byte, "module"))
            attributes = []
            for child in node.children:
                if child.type == "attribute_item":
                    attributes.append(child)
                    walk(child)
                    continue
                if child.type not in ("line_comment", "block_comment"):
                    if any(cls.test_attribute(source[a.start_byte:a.end_byte]) for a in attributes):
                        spans.append((attributes[0].start_byte, child.end_byte, "test"))
                    attributes = []
                walk(child)
            if not node.children:
                leaves.append((node.start_point.row + 1, node.end_point.row + 1,
                               node.type, source[node.start_byte:node.end_byte].decode("utf-8")))

        walk(root)
        categories = {}
        offset = 0
        for line_no, line in enumerate(source.splitlines(keepends=True), 1):
            stripped = line.strip()
            test_span = test_only or any(kind == "test" and start <= offset < end for start, end, kind in spans)
            if not stripped and line_no in literal_lines and not test_span:
                categories[line_no] = "code"
            elif not stripped:
                categories[line_no] = "formatting"
            elif test_only:
                categories[line_no] = "test"
            else:
                covered = []
                for i, byte in enumerate(line):
                    if byte in b" \t\r\n":
                        continue
                    kinds = [kind for start, end, kind in spans if start <= offset + i < end]
                    covered.append("test" if "test" in kinds else kinds[0] if kinds else "code")
                categories[line_no] = next((kind for kind in ("test", "comment", "import", "module")
                                            if covered and all(c == kind for c in covered)), "code")
            offset += len(line)
        return cls(source, categories, leaves, root.has_error)

    def tokens(self, first, end):
        # A token crossing the changed range (e.g. a multiline string) cannot
        # establish formatting equivalence from this fragment alone.
        if any(start < end and stop >= first and (start < first or stop >= end)
               for start, stop, _, _ in self.leaves):
            return None
        return [(kind, text) for start, stop, kind, text in self.leaves if first <= start and stop < end]


@dataclass
class HistoricalCase:
    commit: str
    subject: str
    path: str
    patch: str
    labels: list
    context: dict
    warnings: list

    @classmethod
    def load(cls, commit, subject, path):
        old = History.source(commit + "^", path)
        new = History.source(commit, path)
        if b"\0" in old + new:
            return None
        try:
            patch = History.git("diff", "--no-ext-diff", "--no-renames", "--unified=8", commit + "^", commit, "--", path).decode("utf-8")
            old.decode("utf-8"); new.decode("utf-8")
        except (UnicodeDecodeError, subprocess.CalledProcessError):
            return None
        if "@@ " not in patch or len(patch) > 180_000:
            return None
        proof_old = History.test_proof(commit + "^", path)
        proof_new = History.test_proof(commit, path)
        rust = path.endswith(".rs")
        sides = {"old": SourceLabels.rust(old, bool(proof_old)), "new": SourceLabels.rust(new, bool(proof_new))} if rust else {}
        if any(side.parse_error for side in sides.values()):
            return None
        global_kind = ("lockfile" if path.endswith(".lock") else
                       "documentation" if (path.startswith(("docs/", "plan/")) or Path(path).name in ("README.md", "CHANGELOG.md")) and path.endswith(".md") else "")
        labels, group = [], []
        old_line = new_line = 0
        old_first = new_first = 0

        def flush():
            nonlocal group
            if not group:
                return
            old_changed = [x for x in group if x["side"] == "old"]
            new_changed = [x for x in group if x["side"] == "new"]
            old_tokens = sides["old"].tokens(old_first, old_line) if rust else None
            new_tokens = sides["new"].tokens(new_first, new_line) if rust else None
            same_tokens = rust and old_changed and new_changed and old_tokens is not None and old_tokens == new_tokens
            for item in group:
                kind = global_kind or (sides[item["side"]].categories.get(item["line"], "code") if rust else "code")
                if same_tokens and kind == "code":
                    kind = "formatting"
                item.update(significance=int(kind == "code"), category=kind,
                            reason=("Changed executable code, schema, build/configuration, or runtime prompt is in scope." if kind == "code" else f"Reviewer explicitly excludes {kind}-only edits; syntax and source scope establish this line's category."))
                labels.append(item)
            group = []

        for line in patch.splitlines():
            match = re.match(r"@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@", line)
            if match:
                flush(); old_line, new_line = map(int, match.groups())
            elif line.startswith(("---", "+++")):
                continue
            elif line.startswith("-") or line.startswith("+"):
                if not group:
                    old_first, new_first = old_line, new_line
                side = "old" if line[0] == "-" else "new"
                group.append({"side": side, "line": old_line if side == "old" else new_line})
                old_line += side == "old"; new_line += side == "new"
            elif line.startswith(" "):
                flush(); old_line += 1; new_line += 1
        flush()
        if not labels or len(labels) > 1800:
            return None
        warnings = []
        if rust and (Path(path).name == "tests.rs" or path.endswith(".tests.rs")) and not (proof_old or proof_new):
            return None
        context = {"old_file_header": old.decode().splitlines()[:12], "new_file_header": new.decode().splitlines()[:12]}
        # Proofs are for annotator audit only, never included in model state.
        if proof_old or proof_new:
            warnings.append("Test scope: " + (proof_new or proof_old))
        return cls(commit, subject, path, patch, labels, context, warnings)

    def record(self, case_id):
        return {"id": case_id, "path": self.path,
                "language": "Rust" if self.path.endswith(".rs") else "Markdown" if self.path.endswith(".md") else "TOML" if self.path.endswith((".toml", ".lock")) else "Text",
                "patch": case_id + ".diff", "origin": f"Exact repository history: {self.commit} ({self.subject}), {self.path}; git diff -U8 against its parent.",
                "purpose": "Historical review-scope classification; syntax-assisted annotation, pending corpus audit.",
                "context": self.context, "labels": self.labels}


class CorpusBuilder:
    def __init__(self, root, limit):
        self.root, self.limit = root, limit

    def run(self):
        self.root.mkdir(parents=True, exist_ok=False)
        commits = History.git("log", "--format=%H%x09%s", "--max-count=70", "21d15023a5ed4039e7940005c178d7e49cea2434").decode().splitlines()
        selected, seen_targets = [], set()
        for index, entry in enumerate(commits):
            commit, subject = entry.split("\t", 1)
            paths = History.git("diff-tree", "--no-commit-id", "--name-only", "-r", commit).decode().splitlines()
            candidates = []
            for path in paths:
                if not (path.endswith((".rs", ".md", ".toml", ".lock")) or Path(path).name == "Makefile"):
                    continue
                if Path(path).name in ("AGENTS.md", "CONTEXT.md"):
                    continue
                case = HistoricalCase.load(commit, subject, path)
                if case:
                    signature = hashlib.sha256("\n".join(line for line in case.patch.splitlines() if line.startswith(("+", "-")) and not line.startswith(("+++", "---"))).encode()).hexdigest()
                    if signature not in seen_targets:
                        candidates.append((case, signature))
            # Cover both labels and size bands without letting a giant refactor dominate.
            buckets = {}
            for case, signature in candidates:
                positive = any(x["significance"] for x in case.labels)
                bucket = (positive, len(case.patch) >= 12_000)
                buckets.setdefault(bucket, []).append((case, signature))
            chosen = []
            for bucket in sorted(buckets):
                options = sorted(buckets[bucket], key=lambda x: hashlib.sha256((commit + x[0].path).encode()).hexdigest())
                chosen.extend(options[:2 if bucket[0] else 1])
            for case, signature in chosen[:5]:
                if signature in seen_targets:
                    continue
                seen_targets.add(signature); selected.append((case, index // 3))
            if len(selected) >= self.limit:
                break
        selected = selected[:self.limit]
        cases, audit = [], []
        for index, (case, group) in enumerate(selected):
            case_id = f"h{index:03d}"
            (self.root / (case_id + ".diff")).write_text(case.patch)
            cases.append(case.record(case_id))
            split = "validation" if group % 3 == 1 else "development"
            audit.append({"id": case_id, "commit": case.commit, "commit_group": group, "split": split,
                          "path": case.path, "subject": case.subject, "lines": len(case.labels),
                          "significant_lines": sum(x["significance"] for x in case.labels),
                          "categories": dict(Counter(x["category"] for x in case.labels)), "annotation_evidence": case.warnings})
        (self.root / "labels.json").write_text(json.dumps({"schema_version": 1, "label_policy": "Reviewer policy: comments/imports/module declarations/formatting/test-only/prose docs/generated/lockfiles are excluded. All other changed code or runtime prompts remain in scope. Historical source syntax is annotated before Jev calls; independent audit is recorded separately.", "cases": cases}, indent=2) + "\n")
        (self.root / "audit.json").write_text(json.dumps(audit, indent=2) + "\n")
        print(json.dumps({"cases": len(cases), "commits": len({x["commit"] for x in audit}), "lines": sum(x["lines"] for x in audit), "splits": dict(Counter(x["split"] for x in audit)), "positive_cases": sum(x["significant_lines"] > 0 for x in audit)}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--limit", type=int, default=180)
    args = parser.parse_args()
    CorpusBuilder(args.output, args.limit).run()
