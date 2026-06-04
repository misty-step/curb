#!/usr/bin/env python3
"""Validate Curb NDJSON smoke logs without accepting sensitive content."""

from __future__ import annotations

import json
import sys
from pathlib import Path


REQUIRED_FIELDS = {"schema_version", "timestamp", "level", "component", "event", "outcome"}
REQUIRED_EVENTS = {"config_loaded", "server_started", "usage_scan", "health_check", "readiness_check"}
OPTIONAL_REGISTERED_EVENTS = {
    "api_request",
    "notification_attempt",
    "shutdown",
    "source_health_error",
    "stop_decision",
    "stop_rejection",
    "watcher_tick",
}
FORBIDDEN_TEXT = {"Authorization", "Bearer", "token=", "secret-session"}


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: scripts/parse-observability-smoke.py <curb.ndjson>", file=sys.stderr)
        return 2
    path = Path(sys.argv[1])
    events = []
    for line_number, raw in enumerate(path.read_text().splitlines(), start=1):
        if not raw.strip():
            continue
        try:
            event = json.loads(raw)
        except json.JSONDecodeError as error:
            print(f"{path}:{line_number}: invalid json: {error}", file=sys.stderr)
            return 1
        missing = REQUIRED_FIELDS - event.keys()
        if missing:
            print(f"{path}:{line_number}: missing fields: {sorted(missing)}", file=sys.stderr)
            return 1
        if event["schema_version"] != 1:
            print(f"{path}:{line_number}: unsupported schema_version {event['schema_version']}", file=sys.stderr)
            return 1
        events.append(event)

    names = {event["event"] for event in events}
    unknown_events = names - REQUIRED_EVENTS - OPTIONAL_REGISTERED_EVENTS
    if unknown_events:
        print(f"{path}: unknown events: {sorted(unknown_events)}", file=sys.stderr)
        return 1
    missing_events = REQUIRED_EVENTS - names
    if missing_events:
        print(f"{path}: missing events: {sorted(missing_events)}", file=sys.stderr)
        return 1

    encoded = "\n".join(json.dumps(event, sort_keys=True) for event in events)
    leaked = [text for text in FORBIDDEN_TEXT if text in encoded]
    if leaked:
        print(f"{path}: forbidden sensitive text present: {leaked}", file=sys.stderr)
        return 1

    readiness = [event for event in events if event["event"] == "readiness_check"]
    if not readiness:
        print(f"{path}: no readiness_check events", file=sys.stderr)
        return 1
    for event in readiness:
        fields = event.get("fields", {})
        if fields.get("path_template") != "/v1/ready":
            print(f"{path}: readiness event missing path template", file=sys.stderr)
            return 1
        if "duration_ms" not in event:
            print(f"{path}: readiness event missing duration_ms", file=sys.stderr)
            return 1

    health = [event for event in events if event["event"] == "health_check"]
    for event in health:
        fields = event.get("fields", {})
        if fields.get("path_template") != "/v1/health":
            print(f"{path}: health event missing path template", file=sys.stderr)
            return 1
        if "duration_ms" not in event:
            print(f"{path}: health event missing duration_ms", file=sys.stderr)
            return 1

    usage_scans = [event for event in events if event["event"] in {"usage_scan", "watcher_tick"}]
    for event in usage_scans:
        fields = event.get("fields", {})
        for key in (
            "event_count",
            "process_count",
            "source_errors",
            "observed_sessions",
            "policy_warnings",
            "would_stop",
            "stop_blocked",
            "grace_started",
            "grace_pending",
            "stop_attempted",
            "stop_completed",
            "stop_rejected",
            "resumed_sessions",
            "terminated_sessions",
        ):
            if key not in fields:
                print(f"{path}: {event['event']} missing {key}", file=sys.stderr)
                return 1
        if event["event"] == "usage_scan" and "duration_ms" not in event:
            print(f"{path}: usage_scan missing duration_ms", file=sys.stderr)
            return 1

    for event in events:
        if event["event"] in {"stop_decision", "stop_rejection"} and event.get("reason"):
            print(f"{path}: stop event includes operator reason", file=sys.stderr)
            return 1

    for event in events:
        if event["event"] == "shutdown" and event.get("component") != "server":
            print(f"{path}: shutdown event must come from server component", file=sys.stderr)
            return 1

    print(
        "ok observability events",
        len(events),
        ",".join(event["event"] for event in events),
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
