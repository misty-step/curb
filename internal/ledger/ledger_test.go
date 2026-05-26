package ledger

import (
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
