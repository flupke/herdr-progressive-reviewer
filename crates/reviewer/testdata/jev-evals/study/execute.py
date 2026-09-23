"""Paid, resumable Jev execution. Gold labels never enter requests."""

import argparse
import concurrent.futures
import json
import math
import os
import time
import urllib.error
import urllib.request
from pathlib import Path

from corpus import Corpus, digest
from design import Prompts
from timing import RunTiming

URL = "https://api.typesafe.ai/v1/systemone"
WORKERS = 4


class Transport:
    @staticmethod
    def validate(response, questions):
        assert response["model"] == "jev-1.13.0", "unexpected model"
        answers = response["answers"]
        for name, question in questions.items():
            answer = answers[name]
            assert answer["type"] == question["type"], "unexpected answer type"
            values = ([answer["noul"]] if question["type"] == "noul" else
                      [answer["confidence"], *answer["probabilities"].values()])
            assert all(isinstance(v, (int, float)) and math.isfinite(v) and 0 <= v <= 1 for v in values)
            if question["type"] == "choice":
                assert answer["probabilities"].keys() == question["criteria"].keys()
                assert abs(sum(answer["probabilities"].values()) - 1) < 0.02
                assert answer["choice"] in question["criteria"]
        assert isinstance(response["usage"]["input_tokens"], int)

    @classmethod
    def ask(cls, item):
        key, body = item
        start, attempts, error = time.monotonic(), 0, None
        for attempt in range(3):
            attempts += 1
            request = urllib.request.Request(URL, data=json.dumps(body, ensure_ascii=False).encode(),
                headers={"Authorization": "Bearer " + os.environ["TYPESAFE_API_KEY"],
                         "Content-Type": "application/json"}, method="POST")
            try:
                with urllib.request.urlopen(request, timeout=45) as result:
                    response = json.loads(result.read(1024 * 1024))
                cls.validate(response, body["questions"])
                return {"key": key, "response": response, "attempts": attempts,
                        "elapsed_ms": round((time.monotonic() - start) * 1000)}
            except urllib.error.HTTPError as exc:
                error = f"HTTP {exc.code}: {exc.read(1500).decode(errors='replace')}"
                if exc.code not in (429, 500, 502, 503, 504, 529):
                    break
            except (OSError, ValueError, KeyError, AssertionError) as exc:
                error = f"{type(exc).__name__}: {exc}"
            if attempt < 2:
                time.sleep(2 ** (attempt + 1))
        return {"key": key, "error": error, "attempts": attempts,
                "elapsed_ms": round((time.monotonic() - start) * 1000)}


class Execution:
    def __init__(self, plan, output, split, shortlist=None):
        started = time.monotonic()
        self.corpus = Corpus()
        self.targets = [t for t in self.corpus.targets(plan) if t.split == split]
        self.output, self.split = Path(output), split
        self.selection = json.loads(Path(shortlist).read_text()) if shortlist else None
        if split == "validation":
            assert self.selection, "validation requires a frozen development shortlist"
            assert self.selection["corpus_sha256"] == self.corpus.fingerprint(), "stale shortlist"
            allowed = {(c["prompt"], c["profile"], c["budget"]) for c in self.selection["top10"]}
            self.targets = [t for t in self.targets if (t.record["prompt"], t.record["profile"], t.record["budget"]) in allowed]
        self.requests = {t.key: t.record["request"] for t in self.targets if not t.record["oversized"]}
        self.batches = self.batch_requests()
        self.freeze = {"corpus_sha256": self.corpus.fingerprint(), "split": split,
                       "selection": self.selection, "requests": sorted(self.requests),
                       "batches": sorted(self.batches),
                       "questions": Prompts.questions()}
        self.preparation_ms = round((time.monotonic() - started) * 1000)

    def batch_requests(self):
        groups = {}
        for key, body in self.requests.items():
            # Questions are independent in System One. Only identical states may
            # share a development call; validation sends exact finalist requests.
            group = digest(body["state"]) if self.split == "development" else key
            batch = groups.setdefault(group, {"body": {"model": body["model"], "state": body["state"], "questions": {}}, "members": []})
            batch["body"]["questions"].update(body["questions"])
            batch["members"].append(key)
        return {digest(batch["body"]): batch for batch in groups.values()}

    def run(self, dry_run=False, max_new_calls=None):
        print(json.dumps({"split": self.split, "targets": len(self.targets),
                          "unique_requests": len(self.requests),
                          "physical_calls": len(self.batches),
                          "oversized": sum(t.record["oversized"] for t in self.targets)}), flush=True)
        if dry_run:
            return
        assert len(self.batches) <= 10000, "explicit study cap exceeded"
        assert os.environ.get("TYPESAFE_API_KEY", "").strip(), "missing API key"
        self.output.mkdir(parents=True, exist_ok=True)
        manifest = self.output / (self.split + "-manifest.json")
        if manifest.exists():
            assert json.loads(manifest.read_text()) == self.freeze, "cannot resume a changed experiment"
        else:
            manifest.write_text(json.dumps(self.freeze, indent=2) + "\n")
        path = self.output / (self.split + "-responses.jsonl")
        cache = {r["key"]: r for r in map(json.loads, path.read_text().splitlines())} if path.exists() else {}
        pending = [(k, v["body"]) for k, v in self.batches.items() if k not in cache]
        # Stable shuffle avoids confounding file order with provider drift.
        pending.sort(key=lambda item: digest(item[0]))
        if max_new_calls is not None:
            assert max_new_calls > 0
            pending = pending[:max_new_calls]
        print(f"{len(pending)} calls pending; exact payload cache retains completed responses and failures", flush=True)
        if not pending:
            return
        timing = RunTiming(self.output / (self.split + "-timing.jsonl"),
                           preparation_ms=self.preparation_ms, workers=WORKERS,
                           cached=len(cache), scheduled=len(pending))
        try:
            with path.open("a") as stream, concurrent.futures.ThreadPoolExecutor(max_workers=WORKERS) as pool:
                for index, record in enumerate(pool.map(Transport.ask, pending), 1):
                    record["members"] = self.batches[record["key"]]["members"]
                    stream.write(json.dumps(record) + "\n"); stream.flush()
                    timing.keys.append(record["key"])
                    if index % 20 == 0 or index == len(pending):
                        print(f"{index}/{len(pending)} complete", flush=True)
        finally:
            measured = timing.finish()
            print(f"Execution wall time: {measured['execution_wall_ms'] / 1000:.3f}s; "
                  f"plan loading/verification: {self.preparation_ms / 1000:.3f}s; {WORKERS} workers", flush=True)
        print(f"Saved {path}", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--split", choices=("development", "validation"), required=True)
    parser.add_argument("--shortlist", type=Path)
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--max-new-calls", type=int, help="Protocol probe; later runs resume the same full manifest")
    args = parser.parse_args()
    Execution(args.plan, args.output, args.split, args.shortlist).run(args.dry_run, args.max_new_calls)
