package main

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/phaedrus/curb/internal/config"
)

func actionLabel(mode config.Mode) string {
	switch mode {
	case config.ModeEnforcement:
		return "kill agent processes after the limit"
	case config.ModeAlert:
		return "notify only; never kill"
	default:
		return "record only; never kill"
	}
}

func agentLabels(agents []config.Agent) string {
	labels := make([]string, 0, len(agents))
	for _, agent := range agents {
		labels = append(labels, agent.ID)
	}
	return strings.Join(labels, ", ")
}

func shortDuration(d time.Duration) string {
	d = d.Round(time.Second)
	if d < time.Minute {
		return d.String()
	}
	if d < time.Hour {
		return fmt.Sprintf("%dm%02ds", int(d.Minutes()), int(d.Seconds())%60)
	}
	return fmt.Sprintf("%dh%02dm", int(d.Hours()), int(d.Minutes())%60)
}

func relativeTime(t time.Time) string {
	if t.IsZero() {
		return "unknown"
	}
	elapsed := time.Since(t)
	if elapsed < 0 {
		elapsed = 0
	}
	if elapsed < time.Minute {
		return "now"
	}
	if elapsed < time.Hour {
		return fmt.Sprintf("%dm ago", int(elapsed.Minutes()))
	}
	if elapsed < 24*time.Hour {
		return fmt.Sprintf("%dh ago", int(elapsed.Hours()))
	}
	return fmt.Sprintf("%dd ago", int(elapsed.Hours()/24))
}

func tokenCount(n int64) string {
	if n >= 1_000_000 {
		return fmt.Sprintf("%.1fM", float64(n)/1_000_000)
	}
	if n >= 10_000 {
		return fmt.Sprintf("%dk", n/1_000)
	}
	return fmt.Sprintf("%d", n)
}

func shortSessionID(id string) string {
	if id == "" {
		return "-"
	}
	if len(id) <= 18 {
		return id
	}
	return id[:8] + "..." + id[len(id)-6:]
}

func compactHome(path string) string {
	home, err := os.UserHomeDir()
	if err == nil && home != "" && strings.HasPrefix(path, home) {
		return "~" + strings.TrimPrefix(path, home)
	}
	return path
}

func projectLabel(path string) string {
	if path == "" {
		return "-"
	}
	clean := filepath.Clean(path)
	base := filepath.Base(clean)
	parent := filepath.Base(filepath.Dir(clean))
	if parent == "worktrees" || parent == ".codex" || parent == "Development" || parent == "Documents" {
		return base
	}
	if len(base) <= 18 {
		return base
	}
	return base[:15] + "..."
}
