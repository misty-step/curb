package web

import (
	"embed"
	"io/fs"
	"net/http"
)

//go:embed dist/*
var dist embed.FS

func Handler() (http.Handler, error) {
	root, err := fs.Sub(dist, "dist")
	if err != nil {
		return nil, err
	}
	files := http.FileServer(http.FS(root))
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if _, err := root.Open(pathFor(r.URL.Path)); err == nil {
			files.ServeHTTP(w, r)
			return
		}
		r2 := r.Clone(r.Context())
		r2.URL.Path = "/"
		files.ServeHTTP(w, r2)
	}), nil
}

func pathFor(path string) string {
	if path == "" || path == "/" {
		return "index.html"
	}
	return path[1:]
}
