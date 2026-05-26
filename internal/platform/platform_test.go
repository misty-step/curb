package platform

import (
	"context"
	"os/exec"
	"runtime"
	"testing"
	"time"
)

func TestSnapshotTreeAndDescendants(t *testing.T) {
	snap := &Snapshot{
		Processes: map[int32]Process{
			1: {PID: 1},
			2: {PID: 2, PPID: 1},
			3: {PID: 3, PPID: 2},
			4: {PID: 4, PPID: 2},
		},
		Children: map[int32][]int32{
			1: {2},
			2: {3, 4},
		},
	}
	descendants := snap.Descendants(2)
	if len(descendants) != 2 || descendants[0] != 3 || descendants[1] != 4 {
		t.Fatalf("descendants = %#v", descendants)
	}
	tree := snap.Tree(2)
	if len(tree) != 3 || tree[0] != 2 || tree[1] != 3 || tree[2] != 4 {
		t.Fatalf("tree = %#v", tree)
	}
}

func TestCaptureFindsStartedProcess(t *testing.T) {
	cmd := exec.Command("sleep", "5")
	if err := cmd.Start(); err != nil {
		t.Skipf("sleep unavailable: %v", err)
	}
	done := make(chan error, 1)
	go func() { done <- cmd.Wait() }()
	defer func() { _ = cmd.Process.Kill() }()

	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		snap, err := Capture(context.Background())
		if err != nil {
			t.Fatal(err)
		}
		if proc, ok := snap.Processes[int32(cmd.Process.Pid)]; ok {
			if proc.PID == 0 || !proc.StartedOK {
				t.Fatalf("incomplete process identity: %#v", proc)
			}
			return
		}
		time.Sleep(50 * time.Millisecond)
	}
	t.Fatalf("pid %d not found", cmd.Process.Pid)
}

func TestSafeToTerminateRequiresSemanticIdentity(t *testing.T) {
	start := time.Now().Add(-time.Minute)
	snap := &Snapshot{
		Processes: map[int32]Process{
			10: {PID: 10, Create: start, StartedOK: true, Username: "tester"},
			11: {PID: 11, Name: "agent", Create: start, StartedOK: true, Username: "tester"},
			12: {PID: 12, Exe: "/usr/local/bin/agent", Create: start, StartedOK: true},
			13: {PID: 13, Exe: "/usr/local/bin/agent", Create: start, StartedOK: true, Username: "tester"},
		},
	}
	if snap.SafeToTerminate(Process{PID: 10, Create: start, StartedOK: true, Username: "tester"}) {
		t.Fatal("opaque process should not be safe to terminate")
	}
	if snap.SafeToTerminate(Process{PID: 11, Create: start, StartedOK: true, Username: "tester"}) {
		t.Fatal("opaque expected identity should not be safe to terminate")
	}
	if snap.SafeToTerminate(Process{PID: 11, Name: "agent", Create: start, StartedOK: true, Username: "tester"}) {
		t.Fatal("name-only identity should not be safe to terminate")
	}
	if snap.SafeToTerminate(Process{PID: 12, Exe: "/usr/local/bin/agent", Create: start, StartedOK: true}) {
		t.Fatal("missing owner should not be safe to terminate")
	}
	if snap.SafeToTerminate(Process{PID: 13, Exe: "/usr/local/bin/agent", Username: "tester"}) {
		t.Fatal("missing start time should not be safe to terminate")
	}
	if !snap.SafeToTerminate(Process{PID: 13, Exe: "/usr/local/bin/agent", Create: start, StartedOK: true, Username: "tester"}) {
		t.Fatal("strong process identity should be safe to terminate")
	}
	target, ok := snap.TerminationTarget(Process{PID: 13, Exe: "/usr/local/bin/agent", Create: start, StartedOK: true, Username: "tester"})
	if !ok || target.Root().PID != 13 {
		t.Fatalf("termination target = %#v ok=%v", target, ok)
	}
}

func TestTerminationTargetCapturesTreeAndCopiesPIDs(t *testing.T) {
	start := time.Now().Add(-time.Minute)
	snap := &Snapshot{
		Processes: map[int32]Process{
			10: {PID: 10, Name: "agent", Exe: "/usr/local/bin/agent", Username: "tester", Create: start, StartedOK: true},
			11: {PID: 11, Name: "child", PPID: 10, Create: start, StartedOK: true},
			12: {PID: 12, Name: "child", PPID: 10, Create: start, StartedOK: true},
		},
		Children: map[int32][]int32{10: {11, 12}},
	}
	target, ok := snap.TerminationTarget(Process{PID: 10, Exe: "/usr/local/bin/agent", Username: "tester", Create: start, StartedOK: true})
	if !ok {
		t.Fatal("expected termination target")
	}
	pids := target.PIDs()
	if len(pids) != 3 || pids[0] != 10 || pids[1] != 11 || pids[2] != 12 {
		t.Fatalf("pids = %#v", pids)
	}
	pids[0] = 99
	if target.PIDs()[0] != 10 {
		t.Fatalf("termination target exposed mutable PID slice: %#v", target.PIDs())
	}
}

func TestTerminateTreeKillsRealSubprocess(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("windows process termination semantics are covered by cross-build until a Windows runner is available")
	}
	cmd := exec.Command("sleep", "30")
	if err := cmd.Start(); err != nil {
		t.Skipf("sleep unavailable: %v", err)
	}
	done := make(chan error, 1)
	go func() { done <- cmd.Wait() }()
	defer func() { _ = cmd.Process.Kill() }()

	var snap *Snapshot
	var proc Process
	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		captured, err := Capture(context.Background())
		if err != nil {
			t.Fatal(err)
		}
		if found, ok := captured.Processes[int32(cmd.Process.Pid)]; ok && found.HasTerminationIdentity() && found.StartedOK && found.Username != "" {
			snap = captured
			proc = found
			break
		}
		time.Sleep(50 * time.Millisecond)
	}
	if snap == nil {
		t.Fatalf("did not capture strong identity for pid %d", cmd.Process.Pid)
	}
	target, ok := snap.TerminationTarget(proc)
	if !ok {
		t.Fatal("expected termination target")
	}
	result := TerminateTree(context.Background(), target, time.Millisecond)
	if len(result.SoftSignaled) == 0 && len(result.HardSignaled) == 0 {
		t.Fatalf("expected signal result, got %#v", result)
	}
	select {
	case <-done:
	case <-time.After(2 * time.Second):
		t.Fatal("process remained alive after TerminateTree")
	}
}
