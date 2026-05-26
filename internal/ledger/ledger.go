package ledger

import (
	"bufio"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"time"
)

type Event struct {
	Type      string         `json:"type"`
	Seq       int64          `json:"seq"`
	Time      time.Time      `json:"ts"`
	RunID     string         `json:"run_id,omitempty"`
	AgentID   string         `json:"agent_id,omitempty"`
	Mode      string         `json:"mode,omitempty"`
	Message   string         `json:"message,omitempty"`
	Data      map[string]any `json:"data,omitempty"`
	PrevHash  string         `json:"prev_hash,omitempty"`
	EventHash string         `json:"event_hash,omitempty"`
}

type Ledger struct {
	path     string
	seq      int64
	prevHash string
}

func Open(path string) (*Ledger, error) {
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return nil, err
	}
	l := &Ledger{path: path}
	if err := l.loadTail(); err != nil {
		return nil, err
	}
	return l, nil
}

func (l *Ledger) Append(event Event) error {
	if event.Type == "" {
		return errors.New("ledger event type is required")
	}
	l.seq++
	event.Seq = l.seq
	event.Time = time.Now().UTC()
	event.PrevHash = l.prevHash
	canonical, err := json.Marshal(struct {
		Type     string         `json:"type"`
		Seq      int64          `json:"seq"`
		Time     time.Time      `json:"ts"`
		RunID    string         `json:"run_id,omitempty"`
		AgentID  string         `json:"agent_id,omitempty"`
		Mode     string         `json:"mode,omitempty"`
		Message  string         `json:"message,omitempty"`
		Data     map[string]any `json:"data,omitempty"`
		PrevHash string         `json:"prev_hash,omitempty"`
	}{
		Type: event.Type, Seq: event.Seq, Time: event.Time, RunID: event.RunID,
		AgentID: event.AgentID, Mode: event.Mode, Message: event.Message,
		Data: event.Data, PrevHash: event.PrevHash,
	})
	if err != nil {
		return err
	}
	sum := sha256.Sum256(canonical)
	event.EventHash = hex.EncodeToString(sum[:])
	line, err := json.Marshal(event)
	if err != nil {
		return err
	}

	file, err := os.OpenFile(l.path, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0o600)
	if err != nil {
		return err
	}
	defer file.Close()
	if _, err := file.Write(append(line, '\n')); err != nil {
		return err
	}
	l.prevHash = event.EventHash
	return nil
}

func Read(path string) ([]Event, error) {
	file, err := os.Open(path)
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return nil, nil
		}
		return nil, err
	}
	defer file.Close()

	var events []Event
	scanner := bufio.NewScanner(file)
	for scanner.Scan() {
		var event Event
		if err := json.Unmarshal(scanner.Bytes(), &event); err != nil {
			return nil, fmt.Errorf("read ledger: %w", err)
		}
		events = append(events, event)
	}
	return events, scanner.Err()
}

func (l *Ledger) loadTail() error {
	events, err := Read(l.path)
	if err != nil {
		return err
	}
	if len(events) == 0 {
		return nil
	}
	tail := events[len(events)-1]
	l.seq = tail.Seq
	l.prevHash = tail.EventHash
	return nil
}
