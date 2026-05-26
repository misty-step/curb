package api

import (
	"crypto/rand"
	"encoding/hex"
	"errors"
	"os"
	"path/filepath"
	"strings"
)

func LoadOrCreateToken(stateDir string) (string, string, error) {
	path := filepath.Join(stateDir, "api.token")
	existing, err := os.ReadFile(path)
	if err == nil {
		if err := os.Chmod(path, 0o600); err != nil {
			return "", "", err
		}
		token := strings.TrimSpace(string(existing))
		if token == "" {
			return "", "", errors.New("api token file is empty")
		}
		return token, path, nil
	}
	if !os.IsNotExist(err) {
		return "", "", err
	}
	if err := os.MkdirAll(stateDir, 0o700); err != nil {
		return "", "", err
	}
	raw := make([]byte, 32)
	if _, err := rand.Read(raw); err != nil {
		return "", "", err
	}
	token := hex.EncodeToString(raw)
	if err := os.WriteFile(path, []byte(token+"\n"), 0o600); err != nil {
		return "", "", err
	}
	return token, path, nil
}
