package platform

import (
	"context"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"runtime"
	"sort"
	"time"

	"github.com/shirou/gopsutil/v4/process"
)

type Process struct {
	PID       int32     `json:"pid"`
	PPID      int32     `json:"ppid"`
	Name      string    `json:"name"`
	Exe       string    `json:"exe"`
	Cmdline   string    `json:"cmdline"`
	CWD       string    `json:"cwd,omitempty"`
	Username  string    `json:"username,omitempty"`
	Create    time.Time `json:"create_time"`
	BundleID  string    `json:"bundle_id,omitempty"`
	TeamID    string    `json:"team_id,omitempty"`
	CPU       float64   `json:"cpu_percent,omitempty"`
	StartedOK bool      `json:"started_ok"`
}

type Snapshot struct {
	At        time.Time
	Platform  string
	Processes map[int32]Process
	Children  map[int32][]int32
}

type TerminationTarget struct {
	root Process
	tree []int32
}

func Capture(ctx context.Context) (*Snapshot, error) {
	procs, err := process.ProcessesWithContext(ctx)
	if err != nil {
		return nil, err
	}

	snap := &Snapshot{
		At:        time.Now(),
		Platform:  runtime.GOOS,
		Processes: map[int32]Process{},
		Children:  map[int32][]int32{},
	}
	for _, proc := range procs {
		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		default:
		}
		observed, ok := observeProcess(ctx, proc)
		if !ok {
			continue
		}
		enrichProcess(&observed)
		snap.Processes[observed.PID] = observed
		snap.Children[observed.PPID] = append(snap.Children[observed.PPID], observed.PID)
	}
	for parent := range snap.Children {
		sort.Slice(snap.Children[parent], func(i, j int) bool {
			return snap.Children[parent][i] < snap.Children[parent][j]
		})
	}
	return snap, nil
}

func observeProcess(ctx context.Context, proc *process.Process) (Process, bool) {
	var out Process
	out.PID = proc.Pid
	ppid, err := proc.PpidWithContext(ctx)
	if err == nil {
		out.PPID = ppid
	}
	if name, err := proc.NameWithContext(ctx); err == nil {
		out.Name = name
	}
	if exe, err := proc.ExeWithContext(ctx); err == nil {
		out.Exe = exe
	}
	if cmd, err := proc.CmdlineWithContext(ctx); err == nil {
		out.Cmdline = cmd
	}
	if cwd, err := proc.CwdWithContext(ctx); err == nil {
		out.CWD = cwd
	}
	if username, err := proc.UsernameWithContext(ctx); err == nil {
		out.Username = username
	}
	if createdMillis, err := proc.CreateTimeWithContext(ctx); err == nil {
		out.Create = time.UnixMilli(createdMillis)
		out.StartedOK = true
	}
	if cpu, err := proc.CPUPercentWithContext(ctx); err == nil {
		out.CPU = cpu
	}
	if out.Name == "" && out.Exe == "" && out.Cmdline == "" && !out.StartedOK {
		return out, false
	}
	return out, true
}

func (s *Snapshot) Descendants(root int32) []int32 {
	var out []int32
	seen := map[int32]bool{}
	var visit func(pid int32)
	visit = func(pid int32) {
		for _, child := range s.Children[pid] {
			if seen[child] {
				continue
			}
			seen[child] = true
			out = append(out, child)
			visit(child)
		}
	}
	visit(root)
	return out
}

func (s *Snapshot) Tree(root int32) []int32 {
	pids := append([]int32{root}, s.Descendants(root)...)
	sort.Slice(pids, func(i, j int) bool { return pids[i] < pids[j] })
	return pids
}

func (s *Snapshot) SafeToTerminate(expected Process) bool {
	_, ok := s.TerminationTarget(expected)
	return ok
}

func (s *Snapshot) TerminationTarget(expected Process) (TerminationTarget, bool) {
	if !expected.HasTerminationIdentity() {
		return TerminationTarget{}, false
	}
	current, ok := s.Processes[expected.PID]
	if !ok {
		return TerminationTarget{}, false
	}
	if !current.HasTerminationIdentity() {
		return TerminationTarget{}, false
	}
	if !expected.StartedOK || !current.StartedOK || !current.Create.Equal(expected.Create) {
		return TerminationTarget{}, false
	}
	if current.Username == "" || expected.Username == "" || current.Username != expected.Username {
		return TerminationTarget{}, false
	}
	if !sameTerminationIdentity(expected, current) {
		return TerminationTarget{}, false
	}
	if current.PID == int32(os.Getpid()) || current.PID == 1 {
		return TerminationTarget{}, false
	}
	return TerminationTarget{root: current, tree: s.Tree(current.PID)}, true
}

func (t TerminationTarget) Root() Process {
	return t.root
}

func (t TerminationTarget) PIDs() []int32 {
	return append([]int32(nil), t.tree...)
}

func (p Process) HasSemanticIdentity() bool {
	return p.Name != "" || p.Exe != "" || p.Cmdline != "" || p.BundleID != "" || p.TeamID != ""
}

func (p Process) HasTerminationIdentity() bool {
	return p.Exe != "" || p.BundleID != "" || p.TeamID != ""
}

func sameTerminationIdentity(expected Process, current Process) bool {
	matched := false
	if expected.Exe != "" {
		if current.Exe != expected.Exe {
			return false
		}
		matched = true
	}
	if expected.BundleID != "" {
		if current.BundleID != expected.BundleID {
			return false
		}
		matched = true
	}
	if expected.TeamID != "" {
		if current.TeamID != expected.TeamID {
			return false
		}
		matched = true
	}
	return matched
}

type TerminationResult struct {
	SoftSignaled []int32  `json:"soft_signaled,omitempty"`
	HardSignaled []int32  `json:"hard_signaled,omitempty"`
	Gone         []int32  `json:"gone,omitempty"`
	Errors       []string `json:"errors,omitempty"`
}

type NotificationCapability struct {
	Supported bool   `json:"supported"`
	Status    string `json:"status"`
	Message   string `json:"message"`
}

func TerminateTree(ctx context.Context, target TerminationTarget, grace time.Duration) TerminationResult {
	pids := target.PIDs()
	if len(pids) == 0 || target.root.PID == 0 {
		return TerminationResult{Errors: []string{"empty termination target"}}
	}
	sort.Slice(pids, func(i, j int) bool { return pids[i] > pids[j] })

	var result TerminationResult
	for _, pid := range pids {
		if err := softTerminate(pid); err != nil {
			if errors.Is(err, os.ErrProcessDone) {
				result.Gone = append(result.Gone, pid)
				continue
			}
			result.Errors = append(result.Errors, fmt.Sprintf("soft pid %d: %v", pid, err))
			continue
		}
		result.SoftSignaled = append(result.SoftSignaled, pid)
	}

	timer := time.NewTimer(grace)
	select {
	case <-ctx.Done():
		timer.Stop()
		result.Errors = append(result.Errors, ctx.Err().Error())
		return result
	case <-timer.C:
	}

	for _, pid := range pids {
		if !pidAlive(pid) {
			result.Gone = append(result.Gone, pid)
			continue
		}
		if err := hardTerminate(pid); err != nil {
			result.Errors = append(result.Errors, fmt.Sprintf("hard pid %d: %v", pid, err))
			continue
		}
		result.HardSignaled = append(result.HardSignaled, pid)
	}
	return result
}

func softTerminate(pid int32) error {
	return platformSoftTerminate(pid)
}

func hardTerminate(pid int32) error {
	proc, err := os.FindProcess(int(pid))
	if err != nil {
		return err
	}
	return proc.Kill()
}

func pidAlive(pid int32) bool {
	proc, err := process.NewProcess(pid)
	if err != nil {
		return false
	}
	running, err := proc.IsRunning()
	return err == nil && running
}

func Notify(title, message string) error {
	switch runtime.GOOS {
	case "darwin":
		script := fmt.Sprintf("display notification %q with title %q", message, title)
		return exec.Command("osascript", "-e", script).Run()
	case "linux":
		if _, err := exec.LookPath("notify-send"); err != nil {
			return err
		}
		return exec.Command("notify-send", title, message).Run()
	case "windows":
		return fmt.Errorf("windows toast notifications are not implemented")
	default:
		return fmt.Errorf("notifications unsupported on %s", runtime.GOOS)
	}
}

func NotificationCapabilityStatus() NotificationCapability {
	switch runtime.GOOS {
	case "darwin":
		if _, err := exec.LookPath("osascript"); err != nil {
			return NotificationCapability{Supported: false, Status: "unavailable", Message: err.Error()}
		}
		return NotificationCapability{Supported: true, Status: "available", Message: "macOS user notifications available through osascript"}
	case "linux":
		if _, err := exec.LookPath("notify-send"); err != nil {
			return NotificationCapability{Supported: false, Status: "unavailable", Message: err.Error()}
		}
		return NotificationCapability{Supported: true, Status: "available", Message: "Desktop notification command found"}
	case "windows":
		return NotificationCapability{Supported: false, Status: "unsupported", Message: "Windows toast notifications are not implemented"}
	default:
		return NotificationCapability{Supported: false, Status: "unsupported", Message: fmt.Sprintf("notifications unsupported on %s", runtime.GOOS)}
	}
}

func enrichProcess(p *Process) {
	enrichPlatformProcess(p)
}
