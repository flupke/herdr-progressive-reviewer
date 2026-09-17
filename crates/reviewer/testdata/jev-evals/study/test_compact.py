"""Offline checks for fixed-policy scoring, cache resumption, and wall timing."""

import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from compact import ARMS, COMPACT_SCOPE, MODEL, CompactStudy
from compact_render import publish
from corpus import Target
from design import Prompts


class FixtureCorpus:
    def __init__(self, root, targets):
        self.root = root
        self._targets = targets
        self.cases = {name: {"path": name + ".rs"} for name in ("one", "two")}
        self.audit = {"one": {"split": "development"}, "two": {"split": "validation"}}
        self.gold = {name: {(x["side"], x["line"]): x for t in targets if t.record["case"] == name
                            for x in t.labels} for name in self.cases}

    def targets_for(self, plan, families, profiles, budgets):
        assert families == ("checklist",) and profiles == ARMS and budgets == (16000,)
        return self._targets

    def fingerprint(self):
        return "fixture"


class CompactTests(unittest.TestCase):
    def fixture(self, root):
        root = Path(root)
        plan = root / "planned.jsonl"
        plan.write_text("{}\n")
        errata = {"frozen_corpus_sha256": "fixture", "corrections": [{"case": "one", "side": "new", "line": 1,
                   "significance": 0, "category": "test", "reason": "fixture correction"}]}
        (root / "errata.json").write_text(json.dumps(errata))
        labels = {"one": ({"side": "new", "line": 1, "significance": 1},
                          {"side": "new", "line": 2, "significance": 0}),
                  "two": ({"side": "old", "line": 5, "significance": 1},)}
        targets = []
        for arm in ARMS:
            for case, case_labels in labels.items():
                question = Prompts.questions()["checklist"]
                if arm == "unified_compact":
                    question = json.loads(json.dumps(question))
                    question["instructions"]["target"] = COMPACT_SCOPE
                state = {"path": case + ".rs", "language_hint": "Rust", "file_context": "",
                         "diff": "+x"} if arm == "unified_compact" else {"path": case + ".rs", "rows": [{"content": "x"}]}
                body = {"model": MODEL, "state": state, "questions": {"checklist": question}}
                targets.append(Target({"case": case, "profile": arm, "prompt": "checklist", "budget": 16000,
                                       "chunk": 0, "request": body, "estimated_tokens": 100, "oversized": False},
                                      "development" if case == "one" else "validation", case_labels))
        return plan, FixtureCorpus(root, targets)

    def test_resume_preserves_exact_payload_cache_and_case_timing(self):
        with tempfile.TemporaryDirectory() as tmp:
            plan, corpus = self.fixture(tmp)
            with patch("compact.Corpus", return_value=corpus):
                study = CompactStudy(plan)
            run = Path(tmp) / "run"
            def fake_ask(item):
                key, _body = item
                return {"key": key, "attempts": 1, "elapsed_ms": 5, "response": {"model": MODEL,
                    "usage": {"input_tokens": 80}, "answers": {"checklist": {"type": "choice",
                    "choice": "insignificant", "confidence": .99,
                    "probabilities": {"significant": .01, "insignificant": .98, "uncertain": .01}}}}}
            with patch("compact.Transport.ask", side_effect=fake_ask):
                study.run(run, max_new_calls=2)
                study.run(run)
                recorded = {a: (run / f"{a}-sessions.jsonl").read_bytes() for a in ARMS}
                study.run(run)
                self.assertEqual(recorded, {a: (run / f"{a}-sessions.jsonl").read_bytes() for a in ARMS})
            result = study.report(run)
            for arm in ARMS:
                row = result["arms"][arm]
                self.assertTrue(row["complete"])
                self.assertEqual(row["calls"], 2)
                self.assertEqual(row["changed_lines"], 3)
                self.assertEqual(row["false_hidden_lines"], 1)
                self.assertEqual(row["false_hidden_original_lines"], 2)
                self.assertEqual(row["useful_excluded_lines"], 2)
                self.assertEqual(row["measured_input_tokens"], 160)
                self.assertEqual(len({r["key"] for r in map(json.loads, (run / f"{arm}-responses.jsonl").read_text().splitlines())}), 2)
                self.assertIsNotNone(row["wall_ms"])
            docs = Path(tmp) / "docs"
            publish(result, run, docs, ARMS)
            page = (docs / "jev-compact-query.html").read_text()
            self.assertIn("False-hidden significant lines", page)
            self.assertIn("Provider input tokens", page)
            self.assertIn("one.rs", page)
            self.assertIn("API seconds per 1,000", page)
            self.assertIn("https://docs.typesafe.ai/models", page)
            self.assertIn("Protocol and reproduction", (docs / "jev-compact-query.md").read_text())

    def test_failed_request_keeps_lines_required(self):
        with tempfile.TemporaryDirectory() as tmp:
            plan, corpus = self.fixture(tmp)
            with patch("compact.Corpus", return_value=corpus):
                study = CompactStudy(plan)
            run = Path(tmp) / "run"
            with patch("compact.Transport.ask", side_effect=lambda item: {"key": item[0], "attempts": 3,
                     "elapsed_ms": 25, "error": "HTTP 529: private diagnostics"}):
                study.run(run)
            row = study.report(run)["arms"]["rows_legacy"]
            self.assertEqual(row["failures"], 2)
            self.assertEqual(row["not_evaluated_lines"], 3)
            self.assertEqual(row["false_hidden_lines"], 0)
            self.assertEqual(row["useful_excluded_lines"], 0)
            self.assertNotIn("private diagnostics", (run / "rows_legacy-responses.jsonl").read_text())

    def test_mixed_failure_and_oversize_are_not_safe_credit(self):
        with tempfile.TemporaryDirectory() as tmp:
            plan, corpus = self.fixture(tmp)
            compact_two = next(t for t in corpus._targets if t.record["profile"] == "unified_compact" and t.record["case"] == "two")
            compact_two.record["oversized"] = True
            compact_two.record["estimated_tokens"] = 17000
            with patch("compact.Corpus", return_value=corpus):
                study = CompactStudy(plan)
            def mixed(item):
                key, body = item
                if body["state"]["path"] == "two.rs":
                    return {"key": key, "attempts": 3, "elapsed_ms": 25, "error": "timeout"}
                return {"key": key, "attempts": 1, "elapsed_ms": 5, "response": {"model": MODEL,
                    "usage": {"input_tokens": 80}, "answers": {"checklist": {"type": "choice",
                    "choice": "insignificant", "confidence": .99,
                    "probabilities": {"significant": .01, "insignificant": .98, "uncertain": .01}}}}}
            run = Path(tmp) / "run"
            with patch("compact.Transport.ask", side_effect=mixed):
                study.run(run)
            report = study.report(run)
            legacy = report["arms"]["rows_legacy"]
            self.assertEqual((legacy["failures"], legacy["not_evaluated_lines"], legacy["useful_excluded_lines"]), (1, 1, 2))
            compact = report["arms"]["unified_compact"]
            self.assertEqual((compact["oversized_chunks"], compact["not_evaluated_lines"], compact["useful_excluded_lines"]), (1, 1, 2))

    def test_interruption_retains_measured_arm_segment_for_resume(self):
        with tempfile.TemporaryDirectory() as tmp:
            plan, corpus = self.fixture(tmp)
            with patch("compact.Corpus", return_value=corpus):
                study = CompactStudy(plan)
            run = Path(tmp) / "run"
            calls = 0
            def interrupted(item):
                nonlocal calls
                calls += 1
                if calls == 2:
                    raise KeyboardInterrupt
                return {"key": item[0], "attempts": 1, "elapsed_ms": 5,
                        "response": {"model": MODEL, "usage": {"input_tokens": 80},
                        "answers": {"checklist": {"type": "choice", "choice": "significant",
                        "confidence": .99, "probabilities": {"significant": .98,
                        "insignificant": .01, "uncertain": .01}}}}}
            with patch("compact.Transport.ask", side_effect=interrupted), self.assertRaises(KeyboardInterrupt):
                study.run(run)
            first = json.loads((run / "rows_legacy-sessions.jsonl").read_text().splitlines()[0])
            self.assertEqual(first["new_calls"], 1)
            with patch("compact.Transport.ask", side_effect=interrupted):
                study.run(run)
            self.assertTrue(study.report(run)["arms"]["rows_legacy"]["complete"])


if __name__ == "__main__":
    unittest.main()
