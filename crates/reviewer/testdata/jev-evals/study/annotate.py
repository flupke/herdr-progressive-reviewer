"""Apply explicit agent audit contributions to a generated corpus before inference."""

import argparse
import json
from collections import Counter
from pathlib import Path

from design import DATASET


class AnnotationReview:
    def __init__(self, root):
        self.root = Path(root)

    def apply(self, reports):
        dataset = json.loads((self.root / "labels.json").read_text())
        cases = {c["id"]: c for c in dataset["cases"]}
        reviewed, corrections = set(), {}
        contributions = []
        for path in reports:
            report = json.loads(Path(path).read_text())
            contributions.append(report)
            reviewed.update(report["reviewed_cases"])
            for item in report["corrections"]:
                key = item["case"], item["side"], item["line"]
                assert key not in corrections or corrections[key] == item, "conflicting corrections"
                corrections[key] = item
        assert reviewed == cases.keys(), "audit did not cover every case"
        for (case, side, line), correction in corrections.items():
            label = next(x for x in cases[case]["labels"] if x["side"] == side and x["line"] == line)
            label.update({k: correction[k] for k in ("significance", "category", "reason")})
        for case in cases.values():
            case["purpose"] = "Historical review-scope classification; syntax-assisted annotation audited by GPT-6-sol medium before Jev inference."
        (self.root / "labels.json").write_text(json.dumps(dataset, indent=2) + "\n")
        audit = json.loads((self.root / "audit.json").read_text())
        for row in audit:
            labels = cases[row["id"]]["labels"]
            row["significant_lines"] = sum(x["significance"] for x in labels)
            row["categories"] = dict(Counter(x["category"] for x in labels))
        (self.root / "audit.json").write_text(json.dumps(audit, indent=2) + "\n")
        review = {"reviewer_model": "gpt-6-sol", "reasoning_effort": "medium",
                  "reviewed_cases": len(reviewed), "corrected_lines": len(corrections),
                  "contributions": contributions,
                  "limitations": ["Agent-authored and agent-audited labels are not independent human gold labels.",
                    "One repository; source lines, hunks and neighboring commits remain correlated.",
                    "No test execution or Jev output was used to decide labels."]}
        (self.root / "annotation-review.json").write_text(json.dumps(review, indent=2) + "\n")
        print(json.dumps({"reviewed": len(reviewed), "corrections": len(corrections)}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dataset", type=Path, default=DATASET)
    parser.add_argument("reports", type=Path, nargs="+")
    args = parser.parse_args()
    AnnotationReview(args.dataset).apply(args.reports)
