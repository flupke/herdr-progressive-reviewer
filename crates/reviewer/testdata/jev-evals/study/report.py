"""Build the standalone, interactive study report from recorded measurements."""

import argparse
import hashlib
import json
from collections import Counter
from pathlib import Path

from corpus import Corpus, digest
from design import HERE, ROOT, Prompts, METADATA, BUDGETS
from score import Scoring, Rule
from timing import StudyTiming


class Report:
    def __init__(self, run):
        self.run = Path(run)
        self.corpus = Corpus()

    def read(self, name):
        return json.loads((self.run / name).read_text())

    def build(self):
        shortlist = self.read("shortlist.json")
        scores = self.read("development-scores.json")
        validation = self.read("validation-scores.json")
        errata = json.loads((self.corpus.root / "errata.json").read_text())
        assert errata["frozen_corpus_sha256"] == self.corpus.fingerprint(), "erratum belongs to a different corpus"
        assert all(self.corpus.audit[c["case"]]["split"] == "validation" for c in errata["corrections"]), "development errata require a new selection study"
        corrected = Scoring(self.run / "plans", self.run, "validation", errata["corrections"])
        validation_corrected = [corrected.evaluate(r["profile"], r["budget"], Rule(r["prompt"], r["method"], r["threshold"]))
                                for r in shortlist["top10"]]
        windows = []
        for profile in METADATA:
            for budget in BUDGETS:
                cell = [s for s in scores if s["profile"] == profile and s["budget"] == budget]
                complete = [s for s in cell if s["complete"]]
                windows.append(min(complete or cell, key=Scoring.rank))
        records, artifacts = [], {}
        for split in ("development", "validation"):
            path = self.run / (split + "-responses.jsonl")
            records.extend(json.loads(line) for line in path.read_text().splitlines())
            artifacts[path.name] = hashlib.sha256(path.read_bytes()).hexdigest()
            timing_path = self.run / (split + "-timing.jsonl")
            if timing_path.exists():
                artifacts[timing_path.name] = hashlib.sha256(timing_path.read_bytes()).hexdigest()
        responses = {key: r for r in records for key in r["members"]}
        timing = StudyTiming(self.run, self.corpus)
        wanted = {example["key"] for row in shortlist["worst10"] for example in row["examples"]}
        examples = {}
        for line in self.plan_lines():
            row = json.loads(line)
            key = digest(row["request"])
            timing.include(row, key)
            if key not in wanted or key in examples:
                continue
            case = self.corpus.cases[row["case"]]
            examples[key] = {"case": row["case"], "path": case["path"],
                             "origin": case["origin"],
                             "rows": row["request"]["state"]["rows"],
                             "answers": responses[key].get("response", {}).get("answers", {})}
        for split, rows in (("development", shortlist["top10"]), ("development", shortlist["worst10"]),
                            ("development", windows), ("validation", validation_corrected)):
            for row in rows:
                row["timing"] = timing.cell(row, split)
        category_counts = Counter(label["category"] for c in self.corpus.cases.values() for label in c["labels"])
        payload = {**shortlist, "validation_original": validation,
                   "validation": validation_corrected, "errata": errata,
                   "worst_audit": json.loads((self.corpus.root / "worst-examples-audit.json").read_text()),
                   "windows": windows,
                   "timing": timing.summary(),
                   "questions": Prompts.questions(), "examples": examples,
                   "corpus": list(self.corpus.audit.values()), "categories": dict(category_counts),
                   "audit": json.loads((self.corpus.root / "annotation-review.json").read_text()),
                   "usage": {"calls": len(records), "attempts": sum(r["attempts"] for r in records),
                             "failures": sum("error" in r for r in records),
                             "input_tokens": sum(r.get("response", {}).get("usage", {}).get("input_tokens", 0) for r in records)},
                   "artifacts": artifacts, "run_path": str(self.run.relative_to(ROOT))}
        data = json.dumps(payload, ensure_ascii=False, separators=(",", ":")).replace("<", "\\u003c")
        template = (HERE / "report.html").read_text()
        output = ROOT / "docs/jev-history-study.html"
        output.write_text(template.replace("__STUDY_DATA__", data))
        (ROOT / "docs/jev-history-results.json").write_text(json.dumps(payload, indent=2, ensure_ascii=False) + "\n")
        print(output)

    def plan_lines(self):
        for path in Corpus.plan_files(self.run / "plans"):
            with path.open() as stream:
                yield from stream


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run", type=Path, required=True)
    Report(parser.parse_args().run.resolve()).build()
