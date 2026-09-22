"""Read frozen annotations and verify complete, disjoint target ownership."""

import hashlib
import json
from dataclasses import dataclass
from functools import cached_property
from pathlib import Path

from design import DATASET, METADATA, BUDGETS, FAMILIES


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, ensure_ascii=False,
                                     separators=(",", ":")).encode()).hexdigest()


@dataclass(frozen=True)
class Target:
    record: dict
    split: str
    labels: tuple

    @cached_property
    def key(self):
        return digest(self.record["request"])

    @property
    def significant(self):
        return sum(label["significance"] for label in self.labels)


class Corpus:
    def __init__(self, root=DATASET):
        self.root = Path(root)
        self.cases = {c["id"]: c for c in json.loads((self.root / "labels.json").read_text())["cases"]}
        self.audit = {c["id"]: c for c in json.loads((self.root / "audit.json").read_text())}
        assert self.cases.keys() == self.audit.keys()
        self.gold = {name: {(x["side"], x["line"]): x for x in case["labels"]}
                     for name, case in self.cases.items()}
        assert all(len(self.gold[name]) == len(case["labels"]) for name, case in self.cases.items())

    def fingerprint(self):
        checksum = hashlib.sha256((self.root / "labels.json").read_bytes())
        for case in self.cases.values():
            checksum.update((self.root / case["patch"]).read_bytes())
        return checksum.hexdigest()

    @staticmethod
    def plan_files(plan):
        path = Path(plan)
        return sorted(path.glob("*/planned.jsonl")) if path.is_dir() else [path]

    def targets(self, plan):
        targets, seen = [], {}
        for path in self.plan_files(plan):
            assert path.with_name("dataset.sha256").read_text().strip() == self.fingerprint(), "stale plan"
            with path.open() as stream:
                self.read_targets(stream, targets, seen)
        expected = {(case, family, profile, budget) for case in self.cases for family in FAMILIES for profile in METADATA for budget in BUDGETS}
        assert seen.keys() == expected, "missing study cells"
        assert all(lines == self.gold[case].keys() for (case, _, _, _), lines in seen.items()), "missing target lines"
        return targets

    def read_targets(self, stream, targets, seen):
        for line in stream:
            row = json.loads(line)
            case = row["case"]
            group = (case, row["prompt"], row["profile"], row["budget"])
            owned = {(u["side"], i) for u in row["units"] for i in range(u["first"], u["end"])}
            assert owned and not seen.setdefault(group, set()) & owned, (group, "duplicate/empty target")
            seen[group].update(owned)
            labels = tuple(self.gold[case][key] for key in sorted(owned))
            targets.append(Target(row, self.audit[case]["split"], labels))
