//go:build darwin

package platform

import (
	"os/exec"
	"path/filepath"
	"strings"
)

func enrichPlatformProcess(p *Process) {
	app := appBundlePath(p.Exe)
	if app == "" {
		app = appBundlePath(p.Cmdline)
	}
	if app == "" {
		return
	}
	if bundleID, err := exec.Command("plutil", "-extract", "CFBundleIdentifier", "raw", "-o", "-", filepath.Join(app, "Contents", "Info.plist")).Output(); err == nil {
		p.BundleID = strings.TrimSpace(string(bundleID))
	}
	if p.TeamID == "" {
		if out, err := exec.Command("codesign", "-dv", app).CombinedOutput(); err == nil {
			for _, line := range strings.Split(string(out), "\n") {
				if strings.HasPrefix(line, "TeamIdentifier=") {
					p.TeamID = strings.TrimPrefix(line, "TeamIdentifier=")
				}
			}
		}
	}
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
