package web

import (
	"net/http"
	"net/http/httptest"
	"regexp"
	"strings"
	"testing"
)

func TestHandlerServesEmbeddedDashboardAndSPAFallback(t *testing.T) {
	handler, err := Handler()
	if err != nil {
		t.Fatal(err)
	}

	for _, path := range []string{"/", "/sessions/missing"} {
		req := httptest.NewRequest(http.MethodGet, path, nil)
		res := httptest.NewRecorder()
		handler.ServeHTTP(res, req)
		if res.Code != http.StatusOK {
			t.Fatalf("%s status=%d", path, res.Code)
		}
		if !strings.Contains(res.Body.String(), `<div id="root">`) {
			t.Fatalf("%s did not serve app shell: %s", path, res.Body.String())
		}
	}
}

func TestHandlerServesEmbeddedAssets(t *testing.T) {
	handler, err := Handler()
	if err != nil {
		t.Fatal(err)
	}
	req := httptest.NewRequest(http.MethodGet, "/", nil)
	res := httptest.NewRecorder()
	handler.ServeHTTP(res, req)
	assetPath := cssAssetPath(t, res.Body.String())
	req = httptest.NewRequest(http.MethodGet, assetPath, nil)
	res = httptest.NewRecorder()
	handler.ServeHTTP(res, req)
	if res.Code != http.StatusOK {
		t.Fatalf("status=%d body=%s", res.Code, res.Body.String())
	}
	if !strings.Contains(res.Body.String(), "app-shell") {
		t.Fatalf("asset body = %q", res.Body.String())
	}
}

func cssAssetPath(t *testing.T, html string) string {
	t.Helper()
	matches := regexp.MustCompile(`href="([^"]+\.css)"`).FindStringSubmatch(html)
	if len(matches) != 2 {
		t.Fatalf("missing css asset in %s", html)
	}
	return matches[1]
}
