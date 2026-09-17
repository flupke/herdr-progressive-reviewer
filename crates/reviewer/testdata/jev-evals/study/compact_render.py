"""Render the measured compact-query comparison as Markdown, JSON, and HTML."""

import html
import json
from pathlib import Path


def publish(result, output, docs, arms):
    docs = Path(docs)
    docs.mkdir(parents=True, exist_ok=True)
    price_per_million = 0.042
    result["pricing"] = {"checked_date": "2026-09-23", "model": "jev-1.13.0",
                         "usd_per_million_input_tokens": price_per_million,
                         "source": "https://docs.typesafe.ai/models",
                         "estimate_basis": "reported input tokens from successful responses; failed or retried attempts may add unobserved billable usage",
                         "estimated_usd": {arm: round(result["arms"][arm]["measured_input_tokens"] * price_per_million / 1_000_000, 6) for arm in arms}}
    previous, compact = (result["arms"][arm] for arm in arms)
    if all(row["complete"] and not row["failures"] for row in (previous, compact)):
        input_reduction = 100 * (1 - compact["measured_input_tokens"] / previous["measured_input_tokens"])
        previous_validation = previous["splits"]["validation"]
        compact_validation = compact["splits"]["validation"]
        validation_rates = (f'{100 * previous_validation["useful_excluded_lines"] / previous_validation["insignificant_lines"]:.1f}% '
                            f'versus {100 * compact_validation["useful_excluded_lines"] / compact_validation["insignificant_lines"]:.1f}% '
                            "of validation insignificant lines") if previous_validation["insignificant_lines"] and compact_validation["insignificant_lines"] else "no validation insignificant-line denominator"
        outcome = (f'Both arms evaluated {previous["evaluated_lines"]:,} unique changed lines with no failed requests. '
                   f'Corrected-label false-hidden significant lines/cases were {previous["false_hidden_lines"]}/{previous["false_hidden_cases"]} '
                   f'for previous rows and {compact["false_hidden_lines"]}/{compact["false_hidden_cases"]} for compact diff. '
                   f'Useful insignificant lines excluded were {previous["useful_excluded_lines"]:,} versus {compact["useful_excluded_lines"]:,}; '
                   f'development contributed {previous["splits"]["development"]["useful_excluded_lines"]:,} versus '
                   f'{compact["splits"]["development"]["useful_excluded_lines"]:,}, and validation '
                   f'{previous_validation["useful_excluded_lines"]:,} versus '
                   f'{compact_validation["useful_excluded_lines"]:,} '
                   f'({validation_rates}). '
                   f'Compact used {input_reduction:.1f}% fewer reported provider input tokens. '
                   f'Observed whole-arm API wall was {previous["wall_ms"] / 1000:.1f}s versus {compact["wall_ms"] / 1000:.1f}s.')
    else:
        outcome = "The run is incomplete or includes failures; not-evaluated lines remain required."
    (docs / "jev-compact-query-results.json").write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n")
    columns = ("Arm", "Cases", "Evaluated / changed lines", "False hidden lines/cases",
               "Useful excluded", "Not evaluated", "Calls", "Wall time", "Lines/s",
               "Provider input tokens", "Provider output tokens", "Estimated input tokens", "Median/p95 request")
    def values(name, row):
        latency = row["latency"]
        return (name, row["cases"], f'{row["evaluated_lines"]} / {row["changed_lines"]}',
                f'{row["false_hidden_lines"]}/{row["false_hidden_cases"]}', row["useful_excluded_lines"],
                row["not_evaluated_lines"], row["calls"],
                f'{row["wall_ms"] / 1000:.2f}s' if row["wall_ms"] is not None else "unknown",
                row["lines_per_second"] or "unknown", row["measured_input_tokens"],
                row["measured_output_tokens"], row["estimated_tokens"],
                f'{latency["median_ms"]}/{latency["p95_ms"]} ms')
    table = [values(a, result["arms"][a]) for a in arms]
    markdown = ["# Compact Jev query comparison", "", "## Observed result", "", outcome, "", "Fixed model jev-1.13.0, checklist policy, 16k estimator budget, and 0.85 insignificant probability. The same previously used audited 180 historical file diffs (18,997 unique changed old/new lines) are used for every arm; development and validation are descriptive, not independent holdouts.", "", "One sequential HTTP worker per arm, run in fixed arm order. Wall time is observed whole-arm execution, including retries, backoff, and recording overhead; per-case durations are measured separately. Changed-line denominators count each old/new coordinate once; context and overlap are excluded. One run per arm leaves provider-load drift as a timing limitation.", "", "| " + " | ".join(columns) + " |", "|" + "|".join("---" for _ in columns) + "|"]
    markdown += ["| " + " | ".join(map(str, row)) + " |" for row in table]
    markdown += ["", "Observed API seconds per 1,000 evaluated changed lines: " + "; ".join(
        f'{a}: {result["arms"][a]["time_per_1000_lines_ms"] / 1000:.2f}s'
        if result["arms"][a]["time_per_1000_lines_ms"] is not None else f"{a}: unknown" for a in arms) + "."]
    markdown += ["", "Estimated model charge from successful-response input usage: " + "; ".join(
        f'{a}: ${result["pricing"]["estimated_usd"][a]:.4f}' for a in arms)
        + ". The [TypeSafe model rate](https://docs.typesafe.ai/models) was $0.042 per million input tokens on 2026-09-23; output tokens were free. This is not an invoice, and failed or retried attempts could add unobserved billed usage."]
    if all(result["arms"][a]["complete"] and result["arms"][a]["failures"] == 0 for a in arms) and result["arms"][arms[0]]["measured_input_tokens"]:
        old, new = (result["arms"][a] for a in arms)
        reduction = 100 * (1 - new["measured_input_tokens"] / old["measured_input_tokens"])
        markdown += ["", f"Measured provider input tokens fell by {reduction:.1f}% for the compact arm across these requests. This is a token count, not a dollar estimate."]
        if old["wall_ms"] and new["wall_ms"]:
            speed = 100 * (1 - new["wall_ms"] / old["wall_ms"])
            markdown += [f"Observed whole-arm API wall time changed by {speed:+.1f}% (positive means less time for compact). Each arm ran once in fixed order, so timing differences may include provider-load drift."]
    markdown += ["", "Failures and oversized chunks remain required; they never earn exclusion credit. Useful excluded lines count only wholly insignificant chunks. False-hidden scores apply the disclosed h054 erratum; original-label sensitivity is in the JSON. Raw responses and timing are under " + str(output) + "."]
    markdown += ["", f'Against the original frozen labels, previous rows would hide {previous["false_hidden_original_lines"]} significant lines in {previous["false_hidden_original_cases"]} file diff and compact would hide {compact["false_hidden_original_lines"]} in {compact["false_hidden_original_cases"]}. The documented h054 erratum reclassifies two test-only attribute lines; no predictions or thresholds were changed.', "", "## Split scores", "", "| Arm | Split | Changed lines | Significant | False hidden | Useful excluded | Not evaluated | Calls |", "|---|---|---:|---:|---:|---:|---:|---:|"]
    for arm in arms:
        for split, row in result["arms"][arm]["splits"].items():
            markdown.append(f'| {arm} | {split} | {row["changed_lines"]} | {row["significant_lines"]} | {row["false_hidden_lines"]} | {row["useful_excluded_lines"]} | {row["not_evaluated_lines"]} | {row["calls"]} |')
    markdown += ["", "## Where useful filtering changed", "", "These examples have no significant changed lines under the frozen labels. The compact answers remain `insignificant`, but their probabilities cross below the fixed 0.85 exclusion threshold. The observations do not identify why the model changed its probability.", "", "| File diff | Changed lines | Previous p(insignificant) | Compact p(insignificant) | Useful excluded: previous → compact |", "|---|---:|---:|---:|---:|"]
    for detail in result["examples"]["utility_changes"]:
        old_chunks = detail["arms"][arms[0]]["chunks"]
        new_chunks = detail["arms"][arms[1]]["chunks"]
        old_probability = ", ".join(str(c["insignificant_probability"]) for c in old_chunks)
        new_probability = ", ".join(str(c["insignificant_probability"]) for c in new_chunks)
        markdown.append(f'| {detail["case"]}: {detail["path"]} | {detail["changed_lines"]} | {old_probability} | {new_probability} | {detail["arms"][arms[0]]["useful_excluded_lines"]} → {detail["arms"][arms[1]]["useful_excluded_lines"]} |')
    for detail in result["examples"]["utility_changes"]:
        markdown += ["", f'Excerpt from frozen {detail["case"]} diff:', "", "~~~~diff", "\n".join(detail["patch_excerpt"]), "~~~~"]
    markdown += ["", "## Planning and inference", "", "Planner times were measured during Rust plan export, separately from HTTP inference. Corpus loading is shared once across arms. These are experiment preparation measurements, not part of the whole-arm HTTP wall time.", "", "| Arm | Planning | Inference wall | Combined (excluding shared corpus load) |", "|---|---:|---:|---:|"]
    for arm in arms:
        row = result["arms"][arm]
        fmt = lambda ms: f"{ms / 1000:.3f}s" if ms is not None else "unknown"
        markdown.append(f'| {arm} | {fmt(row["planning_ms"])} | {fmt(row["wall_ms"])} | {fmt(row["wall_plus_planning_ms"])} |')
    markdown += ["", "Planning intervals came from the Rust test export in a debug build. The combined column adds two separately measured phases; it is not a directly timed production run. Shared corpus-load time is recorded in the JSON provenance."]
    markdown += ["", "## File-diff size bins", "", "| Changed lines per file diff | File diffs | Unique changed lines | Previous rows wall | Compact wall |", "|---|---:|---:|---:|---:|"]
    for size in ("1–10", "11–50", "51–200", "201+"):
        old = result["arms"][arms[0]]["size_bins"].get(size)
        new = result["arms"][arms[1]]["size_bins"].get(size)
        if old and new:
            fmt = lambda ms: f"{ms / 1000:.2f}s" if ms is not None else "unknown"
            markdown.append(f'| {size} | {old["cases"]} | {old["changed_lines"]} | {fmt(old["wall_ms"])} | {fmt(new["wall_ms"])} |')
    markdown += ["", "## Per-case timing", "", "| File diff | Changed lines | " + " | ".join(arms) + " |", "|---|---:|" + "|".join("---:" for _ in arms) + "|"]
    for case in result["cases"]:
        arm_rows = case["arms"]
        markdown.append("| " + case["id"] + ": " + case["path"] + " | " + str(arm_rows[arms[0]]["changed_lines"]) + " | " + " | ".join(
            f'{arm_rows[a]["wall_ms"] / 1000:.2f}s / {arm_rows[a]["calls"]} calls' if arm_rows[a]["wall_ms"] is not None else "unknown" for a in arms) + " |")
    markdown += ["", "## Typical measured file diffs", "", "The examples below are the median-duration file diff within each size bin under the previous-row arm. Durations include all calls for that file diff.", "", "| Size bin | File diff | Changed lines | Previous rows | Compact |", "|---|---|---:|---:|---:|"]
    for size in ("1–10", "11–50", "51–200", "201+"):
        candidates = [c for c in result["cases"] if c["arms"][arms[0]]["size_bin"] == size and all(c["arms"][a]["wall_ms"] is not None for a in arms)]
        if candidates:
            typical = sorted(candidates, key=lambda c: c["arms"][arms[0]]["wall_ms"])[len(candidates) // 2]
            markdown.append(f'| {size} | {typical["id"]}: {typical["path"]} | {typical["arms"][arms[0]]["changed_lines"]} | {typical["arms"][arms[0]]["wall_ms"] / 1000:.2f}s | {typical["arms"][arms[1]]["wall_ms"] / 1000:.2f}s |')
    markdown += ["", "## Exact request examples", "", "The h008 pair shows the request shape and estimated token costs for the same historical file diff. The JSON result also includes all h008 chunks and one false-hidden example per affected arm."]
    for arm in arms:
        rows = result["examples"]["paired_requests"][arm]
        if rows:
            markdown += ["", f'### h008 {arm}, chunk {rows[0]["chunk"]}, {rows[0]["estimated_tokens"]} estimated tokens', "", "```json", json.dumps(rows[0]["request"], indent=2, ensure_ascii=False), "```"]
    for arm, example in result["examples"]["false_hidden"].items():
        markdown += ["", f'### False-hidden example: {arm}, {example["case"]} chunk {example["chunk"]}', "", "```json", json.dumps(example, indent=2, ensure_ascii=False), "```"]
    provenance = result["provenance"]
    markdown += ["", "## Protocol and reproduction", "", "The exact previous-winner requests were checked against the frozen export, and both arms use the same audited history corpus. Changed old/new line coordinates are disjoint; context and overlapping rows do not enter throughput denominators. Provider failures and oversized chunks stay required and count as not evaluated. Corrected h054 labels affect scoring only; the original-label false-hidden counts remain in the JSON. Compact target-row indexing was unused here because all 652 compact windows covered full hunks at the 16k budget.", "", f'Corpus SHA-256: `{provenance["corpus_sha256"]}`. Plan SHA-256: `{next(iter(provenance["plan_sha256"].values()))}`. Exact question hashes and response/timing hashes are in [the result JSON](jev-compact-query-results.json).', "", "```sh", "PYTHONDONTWRITEBYTECODE=1 python3 -B crates/reviewer/testdata/jev-evals/study/compact.py audit \\", "  --plan target/jev-compact-two-arm-plan/planned.jsonl \\", "  --previous-plan /tmp/herdr-jev-request-export/planned.jsonl", "PYTHONDONTWRITEBYTECODE=1 python3 -B crates/reviewer/testdata/jev-evals/study/compact.py run \\", "  --plan target/jev-compact-two-arm-plan/planned.jsonl \\", f"  --output {output}", "PYTHONDONTWRITEBYTECODE=1 python3 -B crates/reviewer/testdata/jev-evals/study/compact.py report \\", "  --plan target/jev-compact-two-arm-plan/planned.jsonl \\", f"  --output {output}", "```", "", "Fresh runs require an empty output directory and `TYPESAFE_API_KEY`; the manifest permits resuming only the same exact plan. The measured run used one HTTP worker per arm. Request latency percentiles include failures and retries; whole-arm wall time is measured separately from request-time sums."]
    (docs / "jev-compact-query.md").write_text("\n".join(markdown) + "\n")
    def cell(value, tag="td"):
        return f"<{tag}>{html.escape(str(value))}</{tag}>"

    def html_table(headers, body):
        return ("<div class='table-scroll'><table><thead><tr>" + "".join(cell(h, "th") for h in headers)
                + "</tr></thead><tbody>" + "".join("<tr>" + "".join(cell(v) for v in row) + "</tr>" for row in body)
                + "</tbody></table></div>")

    old, new = (result["arms"][a] for a in arms)
    seconds = lambda value: f"{value / 1000:.2f} s" if value is not None else "unknown"
    metrics = [
        ("False-hidden significant lines / file diffs", f'{old["false_hidden_lines"]} / {old["false_hidden_cases"]}', f'{new["false_hidden_lines"]} / {new["false_hidden_cases"]}'),
        ("Useful insignificant lines excluded", old["useful_excluded_lines"], new["useful_excluded_lines"]),
        ("Evaluated / changed lines", f'{old["evaluated_lines"]} / {old["changed_lines"]}', f'{new["evaluated_lines"]} / {new["changed_lines"]}'),
        ("Provider failures / oversized chunks", f'{old["failures"]} / {old["oversized_chunks"]}', f'{new["failures"]} / {new["oversized_chunks"]}'),
        ("Provider input tokens", old["measured_input_tokens"], new["measured_input_tokens"]),
        ("Provider output tokens", old["measured_output_tokens"], new["measured_output_tokens"]),
        ("Estimated model charge from reported input", f'${result["pricing"]["estimated_usd"][arms[0]]:.4f}', f'${result["pricing"]["estimated_usd"][arms[1]]:.4f}'),
        ("Whole-arm API wall", seconds(old["wall_ms"]), seconds(new["wall_ms"])),
        ("Evaluated lines per second", old["lines_per_second"] or "unknown", new["lines_per_second"] or "unknown"),
        ("API seconds per 1,000 evaluated lines", seconds(old["time_per_1000_lines_ms"]), seconds(new["time_per_1000_lines_ms"])),
        ("Physical requests", old["calls"], new["calls"]),
        ("Median / p95 request latency", f'{old["latency"]["median_ms"]} / {old["latency"]["p95_ms"]} ms', f'{new["latency"]["median_ms"]} / {new["latency"]["p95_ms"]} ms'),
        ("Debug-build planning phase", seconds(old["planning_ms"]), seconds(new["planning_ms"])),
        ("Planning plus API phase sum", seconds(old["wall_plus_planning_ms"]), seconds(new["wall_plus_planning_ms"])),
    ]
    bins = []
    for size in ("1–10", "11–50", "51–200", "201+"):
        older, newer = old["size_bins"].get(size), new["size_bins"].get(size)
        if older and newer:
            bins.append((size, older["cases"], older["changed_lines"], seconds(older["wall_ms"]), seconds(newer["wall_ms"])))
    cases = [(c["id"] + ": " + c["path"], c["arms"][arms[0]]["changed_lines"],
              c["arms"][arms[0]]["calls"], c["arms"][arms[1]]["calls"],
              seconds(c["arms"][arms[0]]["wall_ms"]), seconds(c["arms"][arms[1]]["wall_ms"]))
             for c in result["cases"]]
    utility = []
    utility_excerpts = []
    for detail in result["examples"]["utility_changes"]:
        old_chunks = detail["arms"][arms[0]]["chunks"]
        new_chunks = detail["arms"][arms[1]]["chunks"]
        utility.append((detail["case"] + ": " + detail["path"], detail["changed_lines"],
                        ", ".join(str(c["insignificant_probability"]) for c in old_chunks),
                        ", ".join(str(c["insignificant_probability"]) for c in new_chunks),
                        f'{detail["arms"][arms[0]]["useful_excluded_lines"]} → {detail["arms"][arms[1]]["useful_excluded_lines"]}'))
        utility_excerpts.append("<details><summary>" + html.escape(detail["case"] + " source excerpt") + "</summary><pre>"
                                + html.escape("\n".join(detail["patch_excerpt"])) + "</pre></details>")
    examples = "".join("<details><summary>h008 " + html.escape(a) + " exact request</summary><pre>" + html.escape(json.dumps(result["examples"]["paired_requests"][a][0], indent=2, ensure_ascii=False)) + "</pre></details>" for a in arms if result["examples"]["paired_requests"][a])
    examples += "".join("<details><summary>False-hidden " + html.escape(a) + " exact request and labels</summary><pre>" + html.escape(json.dumps(v, indent=2, ensure_ascii=False)) + "</pre></details>" for a, v in result["examples"]["false_hidden"].items())
    head = "<meta charset='utf-8'><meta name='viewport' content='width=device-width,initial-scale=1'><title>Compact Jev query comparison</title><style>body{font:16px system-ui;max-width:1050px;margin:2rem auto;padding:0 1rem;color:#222;line-height:1.5}table{border-collapse:collapse;width:100%}th,td{border-bottom:1px solid #ccc;padding:.45rem;text-align:right}th:first-child,td:first-child{text-align:left}tr:nth-child(even){background:#f6f6f6}.table-scroll{overflow-x:auto}pre{white-space:pre-wrap;overflow-wrap:anywhere;background:#f6f6f6;padding:1rem}details{margin:1rem 0}summary{cursor:pointer}</style>"
    page = ["<!doctype html><html><head>", head, "</head><body><h1>Compact Jev query comparison</h1>",
            "<p>" + html.escape(outcome) + "</p>",
            f'<p>With the original frozen labels, previous rows would have hidden {previous["false_hidden_original_lines"]} significant lines and compact {compact["false_hidden_original_lines"]}. The documented h054 erratum reclassifies two test-only attribute lines; predictions and thresholds are unchanged.</p>',
            "<p>Fixed 0.85 checklist policy and 16k estimator budget on the same audited 180 historical file diffs and 18,997 unique changed old/new lines. Results are descriptive on a previously used corpus.</p>",
            html_table(("Metric", "Previous rows", "Compact diff"), metrics),
            "<p>API wall time is observed with one sequential HTTP worker per arm and includes retries and recording overhead. Planning was measured separately in a Rust debug test build. One run per arm leaves provider-load drift as a timing limitation. Model charge estimates use the <a href='https://docs.typesafe.ai/models'>TypeSafe rate on 2026-09-23</a> and successful-response input usage; they are not invoices.</p>",
            "<h2>File-diff size bins</h2>", html_table(("Changed lines", "File diffs", "Unique changed lines", "Previous wall", "Compact wall"), bins),
            "<h2>Where useful filtering changed</h2><p>Both examples contain only frozen-label insignificant lines. Probabilities straddled the fixed 0.85 threshold; these observations do not establish why they changed.</p>",
            html_table(("File diff and path", "Changed lines", "Previous p(insignificant)", "Compact p(insignificant)", "Useful excluded: previous → compact"), utility),
            "".join(utility_excerpts),
            "<h2>Every measured file diff</h2>", html_table(("File diff and path", "Changed lines", "Previous calls", "Compact calls", "Previous wall", "Compact wall"), cases),
            "<h2>Exact request examples</h2>", examples,
            "<p>The <a href='jev-compact-query.md'>Markdown report</a> includes split scores, typical file diffs, methods, and reproduction commands. The <a href='jev-compact-query-results.json'>JSON result</a> contains hashes, original-label sensitivity, and raw per-case measurements.</p></body></html>"]
    (docs / "jev-compact-query.html").write_text("".join(page))
