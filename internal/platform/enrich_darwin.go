//go:build darwin

package platform

import (
	"context"
	"os/exec"
	"path/filepath"
	"strings"
	"time"
)

func enrichPlatformProcess(p *Process) {
	app := appBundlePath(p.Exe)
	if app == "" {
		app = appBundlePath(p.Cmdline)
	}
	if app == "" {
		return
	}
	if bundleID, err := boundedOutput("plutil", "-extract", "CFBundleIdentifier", "raw", "-o", "-", filepath.Join(app, "Contents", "Info.plist")); err == nil {
		p.BundleID = strings.TrimSpace(string(bundleID))
	}
	if p.TeamID == "" {
		if out, err := boundedOutput("codesign", "-dv", app); err == nil {
			for _, line := range strings.Split(string(out), "\n") {
				if strings.HasPrefix(line, "TeamIdentifier=") {
					p.TeamID = strings.TrimPrefix(line, "TeamIdentifier=")
				}
			}
		}
	}
}

func boundedOutput(name string, args ...string) ([]byte, error) {
	ctx, cancel := context.WithTimeout(context.Background(), 200*time.Millisecond)
	defer cancel()
	return exec.CommandContext(ctx, name, args...).CombinedOutput()
}

func appBundlePath(path string) string {
	idx := strings.Index(path, ".app/")
	if idx < 0 {
		if strings.HasSuffix(path, ".app") {
			return path
		}
		return ""
	}
	return path[:idx+len(".app")]
}
