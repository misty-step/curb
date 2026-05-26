package service

import (
	"context"
	"sync"
	"testing"
	"time"
)

func TestSnapshotCacheServesCachedSnapshotWithoutRebuilding(t *testing.T) {
	calls := 0
	cache := NewSnapshotCache(func(context.Context) (Snapshot, error) {
		calls++
		return Snapshot{Overview: Overview{Status: "OK"}}, nil
	})

	if _, err := cache.Current(context.Background()); err != ErrSnapshotUnavailable {
		t.Fatalf("current before refresh err = %v", err)
	}
	if err := cache.Refresh(context.Background()); err != nil {
		t.Fatal(err)
	}
	first, err := cache.Current(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	second, err := cache.Current(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if first.Overview.Status != "OK" || second.Overview.Status != "OK" {
		t.Fatalf("snapshots = %#v %#v", first, second)
	}
	if calls != 1 {
		t.Fatalf("build calls = %d", calls)
	}
}

func TestSnapshotCacheAnnotatesOverviewDeltaFromLastSuccessfulSnapshot(t *testing.T) {
	start := time.Date(2026, 5, 22, 12, 0, 0, 0, time.UTC)
	nextStarted := start.Add(time.Minute)
	snapshots := []Snapshot{
		{
			Overview: Overview{
				Status: "OK",
				Sources: []SourceHealth{
					{Provider: "codex", Events: 1},
					{Provider: "claude", Events: 1, Error: "permission denied"},
				},
			},
			Agents: []AgentView{
				{ID: "codex", PID: 10, ProcessStartedAt: &start},
			},
			Sessions: []SessionView{
				{Key: "codex:old", ID: "old", UsageState: "quiet"},
			},
			Turns: []TurnView{
				{ID: "turn-1", Provider: "codex", SessionKey: "codex:old", At: start, TotalTokens: 100},
			},
		},
		{
			Overview: Overview{
				Status: "WATCH",
				Sources: []SourceHealth{
					{Provider: "codex", Events: 2, Error: "schema changed"},
					{Provider: "claude", Events: 1, Error: "permission denied"},
				},
			},
			Agents: []AgentView{
				{ID: "codex", PID: 10, ProcessStartedAt: &nextStarted},
			},
			Sessions: []SessionView{
				{Key: "codex:old", ID: "old", UsageState: "warn"},
				{Key: "codex:new", ID: "new", UsageState: "spending"},
			},
			Turns: []TurnView{
				{ID: "turn-1", Provider: "codex", SessionKey: "codex:old", At: start, TotalTokens: 100},
				{ID: "turn-2", Provider: "codex", SessionKey: "codex:new", At: nextStarted, TotalTokens: 250},
			},
		},
	}
	index := 0
	cache := NewSnapshotCache(func(context.Context) (Snapshot, error) {
		snapshot := snapshots[index]
		index++
		return snapshot, nil
	})

	if err := cache.Refresh(context.Background()); err != nil {
		t.Fatal(err)
	}
	first, err := cache.Current(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if first.Overview.Changes != (OverviewDelta{}) {
		t.Fatalf("first refresh delta = %#v", first.Overview.Changes)
	}
	if err := cache.Refresh(context.Background()); err != nil {
		t.Fatal(err)
	}
	second, err := cache.Current(context.Background())
	if err != nil {
		t.Fatal(err)
	}

	want := OverviewDelta{
		NewSessions:          1,
		SessionsWithNewTurns: 1,
		TokensAdded:          250,
		NewAlerts:            1,
		AgentsStarted:        1,
		AgentsEnded:          1,
		SourceErrors:         1,
	}
	if second.Overview.Changes != want {
		t.Fatalf("delta = %#v, want %#v", second.Overview.Changes, want)
	}
}

func TestSnapshotCacheDeltaIgnoresReorderedSnapshot(t *testing.T) {
	start := time.Date(2026, 5, 22, 12, 0, 0, 0, time.UTC)
	snapshots := []Snapshot{
		{
			Overview: Overview{Status: "OK", Sources: []SourceHealth{{Provider: "codex", Events: 2}}},
			Agents: []AgentView{
				{ID: "codex-a", PID: 10, ProcessStartedAt: &start},
				{ID: "codex-b", PID: 11, ProcessStartedAt: &start},
			},
			Sessions: []SessionView{
				{Key: "codex:a", ID: "a", UsageState: "spending"},
				{Key: "codex:b", ID: "b", UsageState: "quiet"},
			},
			Turns: []TurnView{
				{ID: "turn-a", Provider: "codex", SessionKey: "codex:a", At: start, TotalTokens: 100},
				{ID: "turn-b", Provider: "codex", SessionKey: "codex:b", At: start, TotalTokens: 200},
			},
		},
		{
			Overview: Overview{Status: "OK", Sources: []SourceHealth{{Provider: "codex", Events: 2}}},
			Agents: []AgentView{
				{ID: "codex-b", PID: 11, ProcessStartedAt: &start},
				{ID: "codex-a", PID: 10, ProcessStartedAt: &start},
			},
			Sessions: []SessionView{
				{Key: "codex:b", ID: "b", UsageState: "quiet"},
				{Key: "codex:a", ID: "a", UsageState: "spending"},
			},
			Turns: []TurnView{
				{ID: "turn-b", Provider: "codex", SessionKey: "codex:b", At: start, TotalTokens: 200},
				{ID: "turn-a", Provider: "codex", SessionKey: "codex:a", At: start, TotalTokens: 100},
			},
		},
	}
	index := 0
	cache := NewSnapshotCache(func(context.Context) (Snapshot, error) {
		snapshot := snapshots[index]
		index++
		return snapshot, nil
	})

	if err := cache.Refresh(context.Background()); err != nil {
		t.Fatal(err)
	}
	if err := cache.Refresh(context.Background()); err != nil {
		t.Fatal(err)
	}
	current, err := cache.Current(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if current.Overview.Changes != (OverviewDelta{}) {
		t.Fatalf("delta = %#v", current.Overview.Changes)
	}
}

func TestSnapshotCacheSerializesRefreshBuilds(t *testing.T) {
	var mu sync.Mutex
	inFlight := 0
	maxInFlight := 0
	cache := NewSnapshotCache(func(context.Context) (Snapshot, error) {
		mu.Lock()
		inFlight++
		if inFlight > maxInFlight {
			maxInFlight = inFlight
		}
		mu.Unlock()

		time.Sleep(20 * time.Millisecond)

		mu.Lock()
		inFlight--
		mu.Unlock()
		return Snapshot{Overview: Overview{Status: "OK"}}, nil
	})

	var wg sync.WaitGroup
	wg.Add(2)
	for i := 0; i < 2; i++ {
		go func() {
			defer wg.Done()
			if err := cache.Refresh(context.Background()); err != nil {
				t.Errorf("refresh err = %v", err)
			}
		}()
	}
	wg.Wait()

	if maxInFlight != 1 {
		t.Fatalf("max concurrent builds = %d", maxInFlight)
	}
}
