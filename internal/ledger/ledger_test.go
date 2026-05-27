package ledger

import (
	"encoding/json"
	"path/filepath"
	"testing"
)

func TestAppendAndReadHashChain(t *testing.T) {
	path := filepath.Join(t.TempDir(), "runs.ndjson")
	l, err := Open(path)
	if err != nil {
		t.Fatal(err)
	}
	if err := l.Append(Event{Type: "run_started", RunID: "run_a"}); err != nil {
		t.Fatal(err)
	}
	if err := l.Append(Event{Type: "run_stopped", RunID: "run_a"}); err != nil {
		t.Fatal(err)
	}
	events, err := Read(path)
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 2 {
		t.Fatalf("events = %d", len(events))
	}
	if events[1].PrevHash != events[0].EventHash {
		t.Fatal("hash chain did not link")
	}
}

func TestAppendEnrichesEventsWithMetadata(t *testing.T) {
	path := filepath.Join(t.TempDir(), "runs.ndjson")
	l, err := OpenWithOptions(path, Options{Metadata: map[string]any{"machine_id": "machine_test"}})
	if err != nil {
		t.Fatal(err)
	}
	if err := l.Append(Event{Type: "usage_warning", Data: map[string]any{"machine_id": "explicit", "session": "s1"}}); err != nil {
		t.Fatal(err)
	}
	if err := l.Append(Event{Type: "usage_warning"}); err != nil {
		t.Fatal(err)
	}
	events, err := Read(path)
	if err != nil {
		t.Fatal(err)
	}
	if events[0].Data["machine_id"] != "explicit" {
		t.Fatalf("explicit machine id overwritten: %#v", events[0].Data)
	}
	if events[1].Data["machine_id"] != "machine_test" {
		t.Fatalf("metadata missing: %#v", events[1].Data)
	}
}

func TestAppendRedactsSensitiveDataFields(t *testing.T) {
	path := filepath.Join(t.TempDir(), "runs.ndjson")
	l, err := Open(path)
	if err != nil {
		t.Fatal(err)
	}
	if err := l.Append(Event{Type: "usage_warning", Data: map[string]any{
		"prompt": "secret prompt",
		"nested": map[string]any{
			"response": "secret response",
			"safe":     "metadata",
		},
		"items": []any{map[string]any{"file_contents": "secret file"}},
	}}); err != nil {
		t.Fatal(err)
	}
	events, err := Read(path)
	if err != nil {
		t.Fatal(err)
	}
	data := events[0].Data
	if data["prompt"] != "[redacted]" {
		t.Fatalf("prompt was not redacted: %#v", data)
	}
	nested := data["nested"].(map[string]any)
	if nested["response"] != "[redacted]" || nested["safe"] != "metadata" {
		t.Fatalf("nested data = %#v", nested)
	}
	items := data["items"].([]any)
	item := items[0].(map[string]any)
	if item["file_contents"] != "[redacted]" {
		t.Fatalf("item data = %#v", item)
	}
}

func TestAppendCallsHookAfterLocalWrite(t *testing.T) {
	path := filepath.Join(t.TempDir(), "runs.ndjson")
	var received Event

	l, err := OpenWithOptions(path, Options{
		Metadata: map[string]any{"machine_id": "machine_test"},
		AfterAppend: func(_ Event, line []byte) {
			if err := json.Unmarshal(line, &received); err != nil {
				t.Fatalf("decode appended event: %v", err)
			}
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	if err := l.Append(Event{Type: "usage_warning"}); err != nil {
		t.Fatal(err)
	}
	events, err := Read(path)
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 1 {
		t.Fatalf("local events = %d", len(events))
	}
	if received.Type != "usage_warning" || received.Data["machine_id"] != "machine_test" {
		t.Fatalf("hook event = %#v", received)
	}
}
