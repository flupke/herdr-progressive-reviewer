"""Opt-in, paid prompt comparison over frozen Jev candidate requests.

The baseline run supplies production-shaped candidate state. Gold labels and
curation never enter a provider request. Use --dry-run to validate the plan.
"""

import argparse
import copy
import hashlib
import json
import os
import time
import urllib.error
import urllib.request
import uuid
from dataclasses import dataclass
from pathlib import Path


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
MODEL_URL = "https://api.typesafe.ai/v1/systemone"

PROMPTS = {
    "semantic_safety": {
        "instructions": (
            "Classify only the changed target lines, using unchanged lines to understand them. "
            "The reviewer excludes whole blocks consisting only of comments, imports, use/re-export "
            "or module declarations, formatting, test-only code, prose docs, generated output, "
            "or lockfile changes. These categories are excluded even if an import or test can "
            "change behavior. If any target edit is outside those categories, choose significant, "
            "including a changed return value, serialized-data contract, runtime text, or review rule. "
            "For a mixed block choose significant. If category membership is unclear, choose uncertain. "
            "Judge ONLY rows with target=true; all other rows are context, even if added or "
            "deleted. Old changed lines were deleted; new changed lines were added."
        ),
        "criteria": {
            "significant": "At least one target edit is outside the reviewer's excluded categories.",
            "insignificant": "Every target edit belongs to an excluded category, including behavior-changing imports.",
            "uncertain": "The supplied source cannot establish the category of every target edit.",
        },
    },
    "category_nouls": {
        "is_comment_diff": {
            "type": "noul",
            "instructions": "Are ALL edits in rows with target=true exclusively comments or documentation, with no executable, attribute, or declaration edit? Other rows are context, even if changed.",
        },
        "is_import_diff": {
            "type": "noul",
            "instructions": "Are ALL edits in rows with target=true exclusively import statements, use or pub use re-exports, or mod/pub mod declarations? Include changed import targets and aliases. Other rows are context, even if changed.",
        },
        "is_formatting_diff": {
            "type": "noul",
            "instructions": "Are ALL edits in rows with target=true exclusively formatting or syntactic presentation that preserves the same executable behavior, data contract, and user-facing text? Other rows are context, even if changed.",
        },
    },
}


@dataclass(frozen=True)
class Candidate:
    case: str
    split: str
    chunk: int
    body: dict
    units: list
    significant: bool

    def request_body(self, variant: str):
        body = copy.deepcopy(self.body)
        if variant == "category_nouls":
            body["questions"] = PROMPTS[variant]
        elif variant != "baseline":
            body["questions"]["significance"].update(PROMPTS[variant])
        return body

@dataclass(frozen=True)
class Corpus:
    roles: dict
    gold: dict

    @classmethod
    def load(cls):
        labels = json.loads((HERE / "labels.json").read_text())
        curation = json.loads((HERE / "curation.json").read_text())
        roles = {case: role for role in ("development", "holdout") for case in curation[role]}
        excluded = curation["excluded_from_prompt_tuning"]
        all_ids = {case["id"] for case in labels["cases"]}
        assert len(roles) == len(curation["development"]) + len(curation["holdout"])
        assert not roles.keys() & excluded.keys()
        assert roles.keys() | excluded.keys() == all_ids
        gold = {
            case["id"]: {(item["side"], item["line"]): item["significance"] for item in case["labels"]}
            for case in labels["cases"]
        }
        return cls(roles, gold)

    def candidates(self, plan: Path):
        selected = []
        seen = {case: set() for case in self.roles}
        for raw in plan.read_text().splitlines():
            row = json.loads(raw)
            if ("request" not in row or row.get("strategy") != "recursive"
                    or row.get("budget") != 14000 or row.get("repeat") != 0
                    or row.get("case") not in self.roles):
                continue
            assert not row.get("oversized", False), (row["case"], row["chunk"], "oversized target")
            target = [
                self.gold[row["case"]][(unit["side"], line)]
                for unit in row["result"]["units"]
                for line in range(unit["first"], unit["end"])
            ]
            assert target, (row["case"], row["chunk"])
            target_keys = {
                (unit["side"], line)
                for unit in row["result"]["units"]
                for line in range(unit["first"], unit["end"])
            }
            assert not seen[row["case"]] & target_keys, "target line appears twice"
            seen[row["case"]].update(target_keys)
            selected.append(Candidate(
                row["case"], self.roles[row["case"]], row["chunk"], row["request"],
                row["result"]["units"], any(target),
            ))
        assert {item.case for item in selected} == self.roles.keys(), "candidate export is incomplete"
        assert all(seen[case] == set(self.gold[case]) for case in self.roles), "target lines missing from export"
        assert len({(item.case, item.chunk) for item in selected}) == len(selected)
        return selected


def ask(body: dict, key: str):
    request = urllib.request.Request(
        MODEL_URL,
        data=json.dumps(body, ensure_ascii=False).encode(),
        headers={"Authorization": "Bearer " + key, "Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.loads(response.read(1024 * 1024))
    except urllib.error.HTTPError as error:
        raise RuntimeError(f"Jev HTTP {error.code}: {error.read(1000).decode(errors='replace')}") from error


@dataclass(frozen=True)
class Scorecard:
    records: list

    @staticmethod
    def probability(record):
        answers = record["response"]["answers"]
        if record["variant"] == "category_nouls":
            return max(answer["noul"] for answer in answers.values())
        return answers["significance"]["probabilities"]["insignificant"]

    def rows(self):
        summary = []
        for variant in ("baseline", "semantic_safety", "category_nouls"):
            for split in ("development", "holdout"):
                records = [r for r in self.records if r["variant"] == variant and r["split"] == split]
                for threshold in (0.5, 0.7, 0.8, 0.9, 0.95, 0.99):
                    excluded = [r for r in records if self.probability(r) >= threshold]
                    summary.append({
                        "variant": variant, "split": split, "threshold": threshold,
                        "positive": sum(r["significant"] for r in records),
                        "negative": sum(not r["significant"] for r in records),
                        "false_exclusions": sum(r["significant"] for r in excluded),
                        "correct_exclusions": sum(not r["significant"] for r in excluded),
                    })
        return summary

    @classmethod
    def from_file(cls, path: Path):
        return cls([json.loads(line) for line in path.read_text().splitlines()])


@dataclass(frozen=True)
class Experiment:
    plan: Path
    output: Path
    repeats: int

    def run(self, dry_run: bool):
        items = Corpus.load().candidates(self.plan)
        variants = ("baseline", "semantic_safety", "category_nouls")
        planned = len(items) * len(variants) * self.repeats
        assert self.repeats > 0 and planned <= 200, planned
        print(f"{len(items)} curated candidates; {planned} planned Jev calls")
        if dry_run:
            return
        key = os.environ["TYPESAFE_API_KEY"]
        assert key.strip(), "TYPESAFE_API_KEY is empty"
        self.output.mkdir(parents=True, exist_ok=False)
        metadata = {
            "plan": str(self.plan), "model": "jev-1.13.0", "repeats": self.repeats,
            "planned_calls": planned,
            "labels_sha256": hashlib.sha256((HERE / "labels.json").read_bytes()).hexdigest(),
            "curation_sha256": hashlib.sha256((HERE / "curation.json").read_bytes()).hexdigest(),
            "variants": {"baseline": "Production prompt", **PROMPTS},
        }
        (self.output / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
        with (self.output / "responses.jsonl").open("x") as stream:
            calls = 0
            for repeat in range(self.repeats):
                for item in items:
                    for offset in range(len(variants)):
                        variant = variants[(offset + repeat) % len(variants)]
                        body = item.request_body(variant)
                        started = time.monotonic()
                        response = ask(body, key)
                        record = {
                            "case": item.case, "split": item.split, "chunk": item.chunk,
                            "significant": item.significant, "units": item.units,
                            "variant": variant, "repeat": repeat, "request": body,
                            "response": response, "elapsed_ms": round((time.monotonic() - started) * 1000),
                        }
                        stream.write(json.dumps(record, ensure_ascii=False) + "\n")
                        stream.flush()
                        calls += 1
                        if calls % 20 == 0:
                            print(f"{calls}/{planned} complete", flush=True)
        score = Scorecard.from_file(self.output / "responses.jsonl")
        (self.output / "scores.json").write_text(json.dumps(score.rows(), indent=2) + "\n")
        print(f"{planned}/{planned} complete; results: {self.output}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--plan", type=Path, help="Offline Rust export of recursive candidate requests")
    parser.add_argument("--output", type=Path, default=ROOT / "target/jev-prompt-evals" / str(uuid.uuid4()))
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--score-existing", type=Path, help="Score an existing responses.jsonl without Jev calls")
    args = parser.parse_args()
    if args.score_existing:
        print(json.dumps(Scorecard.from_file(args.score_existing).rows(), indent=2))
    else:
        parser.error("--plan is required") if args.plan is None else Experiment(args.plan, args.output, args.repeats).run(args.dry_run)
