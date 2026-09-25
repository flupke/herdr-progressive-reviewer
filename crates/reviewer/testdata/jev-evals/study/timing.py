"""Client-observed request latency and wall time; overlapping calls are not additive wall time."""

import json
import math
import statistics
import time
from datetime import datetime, timezone
from pathlib import Path


class Latency:
    @staticmethod
    def summarize(records):
        records = list({r["key"]: r for r in records}.values())
        values = sorted(r["elapsed_ms"] for r in records if "elapsed_ms" in r)
        assert all(math.isfinite(v) and v >= 0 for v in values)
        return {"calls": len(records), "timed_calls": len(values),
                "attempts": sum(r["attempts"] for r in records),
                "failures": sum("error" in r for r in records),
                "request_time_ms": sum(values) if len(values) == len(records) else None,
                "median_ms": statistics.median(values) if values else None,
                "p95_ms": values[math.ceil(.95 * len(values)) - 1] if values else None,
                "max_ms": max(values) if values else None}


class RunTiming:
    def __init__(self, path, *, preparation_ms, workers, cached, scheduled):
        self.path = Path(path)
        self.started_at = datetime.now(timezone.utc).isoformat()
        self.start = time.monotonic()
        self.preparation_ms = preparation_ms
        self.workers, self.cached, self.scheduled = workers, cached, scheduled
        self.keys = []

    def finish(self):
        record = {"started_at": self.started_at,
                  "finished_at": datetime.now(timezone.utc).isoformat(),
                  "execution_wall_ms": round((time.monotonic() - self.start) * 1000),
                  "preparation_ms": self.preparation_ms, "workers": self.workers,
                  "cached_calls": self.cached, "scheduled_calls": self.scheduled,
                  "completed_calls": len(self.keys), "response_keys": self.keys,
                  "complete": len(self.keys) == self.scheduled}
        with self.path.open("a") as stream:
            stream.write(json.dumps(record) + "\n")
        return record

    @staticmethod
    def summarize(path, records):
        path = Path(path)
        sessions = [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []
        keys = [key for session in sessions for key in session["response_keys"]]
        complete = (bool(sessions) and all(s["complete"] for s in sessions)
                    and len(keys) == len(set(keys)) and set(keys) == {r["key"] for r in records})
        return {"complete": complete,
                "execution_wall_ms": sum(s["execution_wall_ms"] for s in sessions) if complete else None,
                "preparation_ms": sum(s["preparation_ms"] for s in sessions) if complete else None,
                "sessions": sessions, "latency": Latency.summarize(records)}


class StudyTiming:
    def __init__(self, run, corpus):
        self.run, self.corpus = Path(run), corpus
        self.records, self.by_request, self.groups = {}, {}, {}
        for split in ("development", "validation"):
            records = [json.loads(line) for line in (self.run / (split + "-responses.jsonl")).read_text().splitlines()]
            self.records[split] = records
            self.by_request[split] = {key: r for r in records for key in r["members"]}

    def include(self, row, key):
        split = self.corpus.audit[row["case"]]["split"]
        group = (split, row["prompt"], row["profile"], row["budget"])
        if not row["oversized"]:
            self.groups.setdefault(group, set()).add(key)

    def cell(self, row, split):
        keys = self.groups.get((split, row["prompt"], row["profile"], row["budget"]), set())
        records = self.by_request[split]
        result = Latency.summarize(records[key] for key in keys if key in records)
        return {**result, "missing_requests": len(keys - records.keys()),
                "mode": "shared_questions" if split == "development" else "exact_questions"}

    def summary(self):
        return {split: RunTiming.summarize(self.run / (split + "-timing.jsonl"), records)
                for split, records in self.records.items()}
