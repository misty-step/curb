package service

import (
	"crypto/rand"
	"encoding/hex"
	"errors"
	"os"
	"path/filepath"
	"strings"
)

const machineIDFile = "machine-id"

func ensureMachineID(stateDir string) (string, error) {
	if stateDir == "" {
		return "", errors.New("state dir is required")
	}
	path := filepath.Join(stateDir, machineIDFile)
	if content, err := os.ReadFile(path); err == nil {
		id := strings.TrimSpace(string(content))
		if id != "" {
			return id, nil
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return "", err
	}

	id, err := newMachineID()
	if err != nil {
		return "", err
	}
	if err := os.MkdirAll(stateDir, 0o700); err != nil {
		return "", err
	}
	if err := os.WriteFile(path, []byte(id+"\n"), 0o600); err != nil {
		return "", err
	}
	return id, nil
}

func newMachineID() (string, error) {
	var bytes [16]byte
	if _, err := rand.Read(bytes[:]); err != nil {
		return "", err
	}
	return "machine_" + hex.EncodeToString(bytes[:]), nil
}
