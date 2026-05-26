package usagewatch

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"time"
)

type SessionAck struct {
	SessionKey string    `json:"session_key"`
	Reason     string    `json:"reason,omitempty"`
	Until      time.Time `json:"until"`
	CreatedAt  time.Time `json:"created_at"`
}

func WriteSessionAck(stateDir, sessionKey string, extend time.Duration, reason string, now time.Time) (SessionAck, error) {
	if sessionKey == "" {
		return SessionAck{}, errors.New("session key is required")
	}
	if extend <= 0 {
		return SessionAck{}, errors.New("extension must be positive")
	}
	if now.IsZero() {
		now = time.Now()
	}
	ack := SessionAck{
		SessionKey: sessionKey,
		Reason:     reason,
		Until:      now.Add(extend).UTC(),
		CreatedAt:  now.UTC(),
	}
	path := sessionAckPath(stateDir, sessionKey)
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return SessionAck{}, err
	}
	content, err := json.MarshalIndent(ack, "", "  ")
	if err != nil {
		return SessionAck{}, err
	}
	if err := os.WriteFile(path, content, 0o600); err != nil {
		return SessionAck{}, err
	}
	return ack, nil
}

func ReadSessionAck(stateDir, sessionKey string) (SessionAck, bool, error) {
	content, err := os.ReadFile(sessionAckPath(stateDir, sessionKey))
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return SessionAck{}, false, nil
		}
		return SessionAck{}, false, err
	}
	var ack SessionAck
	if err := json.Unmarshal(content, &ack); err != nil {
		return SessionAck{}, false, err
	}
	return ack, true, nil
}

func ActiveSessionAck(stateDir, sessionKey string, now time.Time) (SessionAck, bool, error) {
	ack, ok, err := ReadSessionAck(stateDir, sessionKey)
	if err != nil || !ok {
		return SessionAck{}, false, err
	}
	if now.IsZero() {
		now = time.Now()
	}
	if !now.Before(ack.Until) {
		return SessionAck{}, false, nil
	}
	return ack, true, nil
}

func DeleteSessionAck(stateDir, sessionKey string) error {
	err := os.Remove(sessionAckPath(stateDir, sessionKey))
	if err != nil && !errors.Is(err, os.ErrNotExist) {
		return err
	}
	return nil
}

func sessionAckPath(stateDir, sessionKey string) string {
	sum := sha256.Sum256([]byte(sessionKey))
	return filepath.Join(stateDir, "usage-acks", hex.EncodeToString(sum[:])+".json")
}
