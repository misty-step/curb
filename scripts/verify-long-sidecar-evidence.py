#!/usr/bin/env python3
from __future__ import annotations

import argparse
from collections import Counter
import csv
import json
from pathlib import Path
import re
import sys


SENSITIVE_RE = re.compile(
    r"Authorization|Bearer|prompt|response|screenshot|keystroke|"
    r"file[-_ ]?content|file contents|raw provider|raw_provider|"
    r"provider payload|payload",
    re.IGNORECASE,
)
TEXT_EVIDENCE_SUFFIXES = {
    ".json",
    ".md",
    ".ndjson",
    ".txt",
    ".tsv",
    ".yaml",
    ".yml",
}


def require_path(path: Path) -> None:
    if not path.exists():
        raise SystemExit(f"missing required evidence file: {path}")


def read_events(path: Path) -> list[dict[str, object]]:
    events: list[dict[str, object]] = []
    for line_number, line in enumerate(path.read_text().splitlines(), start=1):
        if not line.strip():
            continue
        try:
            event = json.loads(line)
        except json.JSONDecodeError as exc:
            raise SystemExit(f"{path}:{line_number}: invalid JSON: {exc}") from exc
        if not isinstance(event, dict) or not isinstance(event.get("event"), str):
            raise SystemExit(f"{path}:{line_number}: missing string event field")
        events.append(event)
    return events


def read_tsv(path: Path) -> list[dict[str, str]]:
    with path.open(newline="") as handle:
        return list(csv.DictReader(handle, delimiter="\t"))


def watcher_tick_floor(duration_seconds: int) -> tuple[int, int]:
    ideal = duration_seconds // 6
    minimum = (ideal * 9 + 9) // 10
    return ideal, max(2, minimum)


def count_source_errors(policy_events: list[dict[str, object]]) -> int:
    total = 0
    for event in policy_events:
        fields = event.get("fields", {})
        if isinstance(fields, dict):
            try:
                total += int(fields.get("source_errors", 0))
            except (TypeError, ValueError):
                pass
    return total


def write_summary(evidence_dir: Path, duration_seconds: int) -> dict[str, object]:
    log_path = evidence_dir / "headless-sidecar.ndjson"
    ready_path = evidence_dir / "ready-samples.tsv"
    probe_path = evidence_dir / "probe-latency.tsv"
    resource_path = evidence_dir / "resource-samples.tsv"

    for path in (log_path, ready_path, probe_path, resource_path):
        require_path(path)

    events = read_events(log_path)
    counts = Counter(str(event["event"]) for event in events)
    policy_events = [
        event for event in events if event["event"] in {"usage_scan", "watcher_tick"}
    ]
    policy_durations = [
        event.get("duration_ms", 0)
        for event in policy_events
        if isinstance(event.get("duration_ms", 0), (int, float))
    ]
    ready_samples = read_tsv(ready_path)
    probe_samples = read_tsv(probe_path)
    resource_samples = read_tsv(resource_path)
    latencies = [
        float(row["total_seconds"])
        for row in probe_samples
        if row.get("total_seconds")
    ]
    rss_values = [
        int(row["rss_kb"]) for row in resource_samples if row.get("rss_kb", "").isdigit()
    ]
    cpu_values = [
        float(row["cpu_percent"]) for row in resource_samples if row.get("cpu_percent")
    ]
    ready_statuses = Counter(row["ready_status"] for row in ready_samples)
    ready_codes = Counter(row["status_code"] for row in ready_samples)
    ideal_watcher_ticks, expected_watcher_ticks = watcher_tick_floor(duration_seconds)

    summary = {
        "duration_seconds": duration_seconds,
        "event_count": len(events),
        "counts": dict(sorted(counts.items())),
        "ideal_watcher_ticks": ideal_watcher_ticks,
        "expected_watcher_ticks": expected_watcher_ticks,
        "watcher_tick_minimum_percent": 90,
        "watcher_tick_count": counts.get("watcher_tick", 0),
        "usage_scan_count": counts.get("usage_scan", 0),
        "source_error_total": count_source_errors(policy_events),
        "source_health_error_events": counts.get("source_health_error", 0),
        "ready_statuses": dict(sorted(ready_statuses.items())),
        "ready_status_codes": dict(sorted(ready_codes.items())),
        "snapshot_count": len(ready_samples),
        "max_policy_duration_ms": max(policy_durations) if policy_durations else 0,
        "max_probe_latency_seconds": max(latencies) if latencies else 0,
        "max_rss_kb": max(rss_values) if rss_values else 0,
        "min_rss_kb": min(rss_values) if rss_values else 0,
        "max_cpu_percent": max(cpu_values) if cpu_values else 0,
    }
    (evidence_dir / "long-run-summary.json").write_text(
        json.dumps(summary, indent=2, sort_keys=True) + "\n"
    )
    (evidence_dir / "long-run-summary.txt").write_text(
        f"duration_seconds={summary['duration_seconds']}\n"
        f"events={summary['event_count']}\n"
        f"usage_scan={summary['usage_scan_count']}\n"
        f"watcher_tick={summary['watcher_tick_count']}\n"
        f"ideal_watcher_tick_count={summary['ideal_watcher_ticks']}\n"
        f"expected_watcher_tick_min={summary['expected_watcher_ticks']}\n"
        f"watcher_tick_minimum_percent={summary['watcher_tick_minimum_percent']}\n"
        f"source_error_total={summary['source_error_total']}\n"
        f"source_health_error_events={summary['source_health_error_events']}\n"
        f"ready_statuses={summary['ready_statuses']}\n"
        f"ready_status_codes={summary['ready_status_codes']}\n"
        f"snapshots={summary['snapshot_count']}\n"
        f"max_policy_duration_ms={summary['max_policy_duration_ms']}\n"
        f"max_probe_latency_seconds={summary['max_probe_latency_seconds']}\n"
        f"rss_kb_min={summary['min_rss_kb']}\n"
        f"rss_kb_max={summary['max_rss_kb']}\n"
        f"max_cpu_percent={summary['max_cpu_percent']}\n"
    )
    return summary


def redact_local_paths(evidence_dir: Path, prefixes: list[str]) -> None:
    patterns = [
        re.compile(re.escape(prefix.rstrip("/")) + r"""[^\s"'):<>`]*""")
        for prefix in prefixes
        if prefix
    ]
    if not patterns:
        return

    changed_files = 0
    replacements = 0
    for path in sorted(evidence_dir.rglob("*")):
        if not path.is_file() or path.suffix not in TEXT_EVIDENCE_SUFFIXES:
            continue
        text = path.read_text(errors="ignore")
        redacted = text
        for pattern in patterns:
            redacted, count = pattern.subn("<redacted-local-path>", redacted)
            replacements += count
        if redacted != text:
            path.write_text(redacted)
            changed_files += 1

    marker_count = 0
    if not replacements:
        for path in sorted(evidence_dir.rglob("*")):
            if not path.is_file() or path.suffix not in TEXT_EVIDENCE_SUFFIXES:
                continue
            marker_count += path.read_text(errors="ignore").count("<redacted-local-path>")

    if replacements:
        message = (
            "ok: redacted "
            f"{replacements} local path occurrence(s) across {changed_files} file(s)\n"
        )
    elif marker_count:
        message = (
            "ok: local paths already redacted; found "
            f"{marker_count} redaction marker occurrence(s)\n"
        )
    else:
        message = "ok: no local path occurrences found for configured prefixes\n"
    (evidence_dir / "path-redaction.txt").write_text(message)


def verify_probes(evidence_dir: Path) -> list[str]:
    issues: list[str] = []
    ready_final_status = evidence_dir / "ready-final.status"
    probe_path = evidence_dir / "probe-latency.tsv"
    require_path(ready_final_status)
    require_path(probe_path)

    if ready_final_status.read_text().strip() != "200":
        issues.append("final /v1/ready status was not HTTP 200")

    bad_probes = [
        row
        for row in read_tsv(probe_path)
        if row.get("path") in {"/v1/live", "/v1/health", "/v1/overview"}
        and row.get("status_code") != "200"
    ]
    if bad_probes:
        rendered = ", ".join(
            f"{row.get('timestamp_utc')} {row.get('path')}={row.get('status_code')}"
            for row in bad_probes[:5]
        )
        issues.append(f"sampled live/health/overview probe failure: {rendered}")
    return issues


def verify_redaction(evidence_dir: Path, redaction_token: str | None) -> None:
    log_path = evidence_dir / "headless-sidecar.ndjson"
    require_path(log_path)
    text = log_path.read_text()
    redaction_path = evidence_dir / "redaction-check.txt"
    issues: list[str] = []
    if redaction_token and redaction_token in text:
        issues.append("runtime API token appeared in NDJSON")
    match = SENSITIVE_RE.search(text)
    if match:
        issues.append(f"sensitive marker appeared in NDJSON: {match.group(0)}")
    if issues:
        redaction_path.write_text("\n".join(issues) + "\n")
        raise SystemExit("; ".join(issues))
    token_clause = "token, " if redaction_token else ""
    redaction_path.write_text(
        "ok: no "
        f"{token_clause}auth header, prompt, response, screenshot, keystroke, "
        "file-content, raw-provider, or payload terms in NDJSON\n"
    )


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Verify a long sidecar dogfood evidence packet."
    )
    parser.add_argument("evidence_dir", type=Path)
    parser.add_argument("--duration-seconds", type=int, required=True)
    parser.add_argument("--redaction-token")
    parser.add_argument(
        "--redact-local-path-prefix",
        action="append",
        default=[],
        help="Replace committed evidence paths under this prefix with a placeholder.",
    )
    args = parser.parse_args()

    if args.duration_seconds < 1:
        raise SystemExit("--duration-seconds must be a positive integer")

    evidence_dir = args.evidence_dir
    require_path(evidence_dir)
    summary = write_summary(evidence_dir, args.duration_seconds)
    redact_local_paths(evidence_dir, args.redact_local_path_prefix)
    verify_redaction(evidence_dir, args.redaction_token)

    issues = verify_probes(evidence_dir)
    if summary["usage_scan_count"] < 1:
        issues.append("expected at least one usage_scan event")
    if summary["watcher_tick_count"] < summary["expected_watcher_ticks"]:
        issues.append(
            "expected at least "
            f"{summary['expected_watcher_ticks']} watcher_tick events "
            f"for {args.duration_seconds}s window "
            f"({summary['watcher_tick_minimum_percent']}% of "
            f"{summary['ideal_watcher_ticks']} ideal ticks)"
        )

    verification_path = evidence_dir / "verification.txt"
    if issues:
        verification_path.write_text("failed:\n- " + "\n- ".join(issues) + "\n")
        raise SystemExit("; ".join(issues))
    verification_path.write_text(
        "ok: final readiness/probes, usage scan, watcher tick floor, and "
        "NDJSON redaction checks passed\n"
    )
    print(f"long sidecar evidence ok: {evidence_dir}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
