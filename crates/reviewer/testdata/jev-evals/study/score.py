"""Offline threshold search; validation never selects or reorders finalists."""

import argparse
import json
import math
from collections import defaultdict
from dataclasses import asdict, dataclass
from pathlib import Path

from corpus import Corpus
from design import THRESHOLDS


@dataclass(frozen=True)
class Rule:
    prompt: str
    method: str
    threshold: float

    @classmethod
    def grid(cls):
        for prompt in ("terse", "checklist", "contrast"):
            yield cls(prompt, "argmax", 0.0)
            for method in ("probability", "margin", "probability_times_confidence",
                           "confidence_at_least_0.8", "confidence_at_least_0.95"):
                for threshold in THRESHOLDS:
                    yield cls(prompt, method, threshold)
        for method in ("max", "sum_clipped", "noisy_or", "max_with_veto", "review_complement"):
            for threshold in THRESHOLDS:
                yield cls("categories", method, threshold)

    def value(self, answers):
        if self.prompt == "categories":
            probabilities = [answers[name]["noul"] for name in
                             ("comment", "import", "formatting", "out_of_scope_file")]
            other = answers["requires_review"]["noul"]
            return {"max": max(probabilities), "sum_clipped": min(1, sum(probabilities)),
                    "noisy_or": 1 - math.prod(1 - p for p in probabilities),
                    "max_with_veto": min(max(probabilities), 1 - other),
                    "review_complement": 1 - other}[self.method]
        answer = answers[self.prompt]
        p = answer["probabilities"]
        confidence = answer["confidence"]
        if self.method == "argmax":
            return 1.0 if answer["choice"] == "insignificant" else -1.0
        if self.method.startswith("confidence_at_least_"):
            return p["insignificant"] if confidence >= float(self.method.rsplit("_", 1)[1]) else -1.0
        return {"probability": p["insignificant"],
                "margin": p["insignificant"] - max(p["significant"], p["uncertain"]),
                "probability_times_confidence": p["insignificant"] * confidence}[self.method]


class Scoring:
    def __init__(self, plan, output, split, corrections=()):
        self.corpus = Corpus()
        # Explicit sensitivity analysis only: the on-disk frozen labels and
        # their manifest fingerprint remain unchanged.
        for item in corrections:
            label = self.corpus.gold[item["case"]][item["side"], item["line"]]
            assert item["significance"] in (0, 1) and item["reason"]
            label.update({k: item[k] for k in ("significance", "category", "reason")})
        self.output, self.split = Path(output), split
        self.targets = [t for t in self.corpus.targets(plan) if t.split == split]
        records = [json.loads(line) for line in (self.output / (split + "-responses.jsonl")).read_text().splitlines()]
        self.responses = {key: r for r in records for key in r["members"]}
        assert len({r["key"] for r in records}) == len(records), "duplicate cached responses"
        manifest = json.loads((self.output / (split + "-manifest.json")).read_text())
        assert manifest["corpus_sha256"] == self.corpus.fingerprint(), "labels changed during run"
        assert self.responses.keys() == set(manifest["requests"]), "execution incomplete"
        self.groups = defaultdict(list)
        for target in self.targets:
            self.groups[target.record["prompt"], target.record["profile"], target.record["budget"]].append((target, self.responses.get(target.key)))

    def evaluate(self, profile, budget, rule):
        targets = self.groups[rule.prompt, profile, budget]
        false_cases, excluded_cases, examples = set(), set(), []
        request_keys = set()
        counts = dict(false_lines=0, useful_lines=0, negative_lines=0, significant_lines=0,
                      excluded_chunks=0, false_chunks=0, chunks=len(targets), calls=0,
                      failures=0, oversized=0, estimated_tokens=0,
                      measured_input_tokens=0 if self.split == "validation" else None)
        for target, record in targets:
            counts["negative_lines"] += len(target.labels) - target.significant
            counts["significant_lines"] += target.significant
            if target.record["oversized"]:
                counts["oversized"] += 1
                continue
            assert record is not None, "missing response for this study cell"
            first_request = target.key not in request_keys
            request_keys.add(target.key)
            if first_request:
                counts["calls"] += 1
                counts["estimated_tokens"] += target.record["estimated_tokens"]
            if "error" in record:
                counts["failures"] += 1
                continue  # Provider failures remain required; never safe credit.
            if first_request and counts["measured_input_tokens"] is not None:
                counts["measured_input_tokens"] += record["response"]["usage"]["input_tokens"]
            value = rule.value(record["response"]["answers"])
            if value < rule.threshold:
                continue
            counts["excluded_chunks"] += 1
            if target.significant:
                counts["false_lines"] += target.significant
                counts["false_chunks"] += 1
                false_cases.add(target.record["case"])
                if len(examples) < 5:
                    examples.append({"case": target.record["case"], "chunk": target.record["chunk"],
                                     "key": target.key, "score": value,
                                     "significant_labels": [l for l in target.labels if l["significance"]][:5]})
            else:
                counts["useful_lines"] += len(target.labels)
                excluded_cases.add(target.record["case"])
        return {**asdict(rule), "profile": profile, "budget": budget, "split": self.split,
                "complete": counts["failures"] == 0 and counts["oversized"] == 0,
                **counts, "false_cases": len(false_cases), "useful_cases": len(excluded_cases),
                "useful_lines_per_call": counts["useful_lines"] / max(counts["calls"], 1),
                "examples": examples}

    @staticmethod
    def rank(row):
        # Safety first, then useful work avoided per request. No exclusions cannot win
        # over equally safe useful exclusions. Largest tied threshold is conservative.
        return (row["false_cases"], row["false_lines"], row["failures"] + row["oversized"],
                -row["useful_lines_per_call"], -row["useful_lines"], row["calls"],
                row["estimated_tokens"], -row["threshold"], row["prompt"], row["profile"], row["method"])

    def development(self):
        rows = [self.evaluate(profile, budget, rule) for prompt, profile, budget in sorted(self.groups)
                for rule in Rule.grid() if rule.prompt == prompt]
        # One winner per prompt × metadata × budget, not ten identical threshold rows.
        families = defaultdict(list)
        for row in rows:
            families[row["prompt"], row["profile"], row["budget"]].append(row)
        eligible = [group for group in families.values()
                    if group[0]["failures"] == 0 and group[0]["oversized"] == 0]
        finalists = sorted((min(group, key=self.rank) for group in eligible), key=self.rank)
        assert len(finalists) >= 10, "fewer than ten complete cells; cannot produce the requested shortlist"
        worst = sorted((r for r in rows if r["false_cases"] and not r["failures"] and not r["oversized"]),
                       key=lambda r: (-r["false_cases"], -r["false_lines"], -r["threshold"]))
        distinct_worst, used = [], set()
        for row in worst:
            family = row["prompt"], row["profile"], row["method"]
            if family not in used:
                distinct_worst.append(row); used.add(family)
            if len(distinct_worst) == 10:
                break
        shortlist = {"corpus_sha256": self.corpus.fingerprint(),
                     "ranking": "False-excluded cases, false lines, failed/oversized chunks (all ascending); then useful insignificant lines excluded per request (descending), useful lines, fewer calls, fewer estimated tokens, stricter threshold. One finalist per prompt/metadata/window cell.",
                     "top10": finalists[:10], "worst10": distinct_worst,
                     "ineligible_cells": [{k: g[0][k] for k in ("prompt", "profile", "budget", "failures", "oversized")}
                                          for g in families.values() if g[0]["failures"] or g[0]["oversized"]],
                     "combinations": len(rows)}
        path = self.output / "shortlist.json"
        if path.exists():
            assert json.loads(path.read_text()) == shortlist, "frozen shortlist changed"
        else:
            path.write_text(json.dumps(shortlist, indent=2) + "\n")
        (self.output / "development-scores.json").write_text(json.dumps(rows, indent=2) + "\n")
        return shortlist

    def validation(self):
        shortlist = json.loads((self.output / "shortlist.json").read_text())
        assert shortlist["corpus_sha256"] == self.corpus.fingerprint(), "stale shortlist"
        rows = [self.evaluate(r["profile"], r["budget"], Rule(r["prompt"], r["method"], r["threshold"]))
                for r in shortlist["top10"]]
        (self.output / "validation-scores.json").write_text(json.dumps(rows, indent=2) + "\n")
        return rows


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--split", choices=("development", "validation"), required=True)
    args = parser.parse_args()
    scoring = Scoring(args.plan, args.output, args.split)
    result = scoring.development() if args.split == "development" else scoring.validation()
    print(json.dumps(result, indent=2))
