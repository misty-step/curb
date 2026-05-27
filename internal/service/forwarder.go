package service

import (
	"bytes"
	"context"
	"io"
	"net/http"
	"time"

	"github.com/phaedrus/curb/internal/ledger"
)

const ledgerForwardTimeout = 2 * time.Second

func ledgerForwardHook(rawURL string) ledger.AppendHook {
	if rawURL == "" {
		return nil
	}
	return func(event ledger.Event, line []byte) {
		body := append([]byte(nil), line...)
		go func() {
			ctx, cancel := context.WithTimeout(context.Background(), ledgerForwardTimeout)
			defer cancel()
			req, err := http.NewRequestWithContext(ctx, http.MethodPost, rawURL, bytes.NewReader(body))
			if err != nil {
				return
			}
			req.Header.Set("Content-Type", "application/json")
			req.Header.Set("X-Curb-Event-Type", event.Type)
			res, err := http.DefaultClient.Do(req)
			if err != nil {
				return
			}
			defer res.Body.Close()
			_, _ = io.Copy(io.Discard, res.Body)
		}()
	}
}
