"""Timing accounting must not turn cached, overlapping or missing observations into wall time."""

import json
import tempfile
import unittest
from pathlib import Path

from timing import Latency, RunTiming, StudyTiming


class TimingTests(unittest.TestCase):
    def test_shared_payload_is_counted_once_and_failures_are_included(self):
        first = {"key": "a", "attempts": 1, "elapsed_ms": 100}
        failed = {"key": "b", "attempts": 3, "elapsed_ms": 5000, "error": "timeout"}
        result = Latency.summarize([first, first, failed])
        self.assertEqual((result["calls"], result["attempts"], result["failures"]), (2, 4, 1))
        self.assertEqual(result["request_time_ms"], 5100)
        self.assertEqual(result["median_ms"], 2550)
        self.assertEqual(result["p95_ms"], 5000)

    def test_missing_latency_is_unknown_not_zero(self):
        result = Latency.summarize([{"key": "old", "attempts": 1}])
        self.assertIsNone(result["request_time_ms"])
        self.assertIsNone(result["median_ms"])

    def test_wall_time_requires_complete_sessions_for_every_response(self):
        records = [{"key": key, "elapsed_ms": 900, "attempts": 1} for key in ("a", "b")]
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "timing.jsonl"
            self.assertIsNone(RunTiming.summarize(path, records)["execution_wall_ms"])
            session = {"complete": True, "response_keys": ["a"], "execution_wall_ms": 1000,
                       "preparation_ms": 50}
            path.write_text(json.dumps(session) + "\n")
            self.assertIsNone(RunTiming.summarize(path, records)["execution_wall_ms"])
            session["response_keys"].append("b")
            path.write_text(json.dumps(session) + "\n")
            result = RunTiming.summarize(path, records)
            self.assertEqual(result["execution_wall_ms"], 1000)
            self.assertEqual(result["latency"]["request_time_ms"], 1800)
            self.assertEqual(result["preparation_ms"], 50)
            session["complete"] = False
            path.write_text(json.dumps(session) + "\n")
            self.assertIsNone(RunTiming.summarize(path, records)["execution_wall_ms"])

    def test_cell_deduplicates_shared_calls_without_hiding_missing_requests(self):
        timing = object.__new__(StudyTiming)
        timing.groups = {("development", "checklist", "headers", 16000): {"x", "y", "z"}}
        record = {"key": "shared", "elapsed_ms": 400, "attempts": 1}
        timing.by_request = {"development": {"x": record, "y": record}}
        cell = timing.cell({"prompt": "checklist", "profile": "headers", "budget": 16000}, "development")
        self.assertEqual(cell["calls"], 1)
        self.assertEqual(cell["request_time_ms"], 400)
        self.assertEqual(cell["missing_requests"], 1)
        self.assertEqual(cell["mode"], "shared_questions")


if __name__ == "__main__":
    unittest.main()
