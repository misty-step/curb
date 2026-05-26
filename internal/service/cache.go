package service

import (
	"context"
	"errors"
	"sync"
	"time"
)

var ErrSnapshotUnavailable = errors.New("snapshot unavailable")

type SnapshotBuilder func(context.Context) (Snapshot, error)

type SnapshotCache struct {
	build SnapshotBuilder

	refreshMu sync.Mutex
	mu        sync.RWMutex
	snapshot  Snapshot
	ready     bool
	lastErr   error
}

func NewSnapshotCache(build SnapshotBuilder) *SnapshotCache {
	return &SnapshotCache{build: build}
}

func (c *SnapshotCache) Refresh(ctx context.Context) error {
	c.refreshMu.Lock()
	defer c.refreshMu.Unlock()
	snapshot, err := c.build(ctx)
	c.mu.Lock()
	defer c.mu.Unlock()
	if err != nil {
		c.lastErr = err
		return err
	}
	snapshot = annotateOverviewDelta(c.snapshot, snapshot, c.ready)
	c.snapshot = snapshot
	c.ready = true
	c.lastErr = nil
	return nil
}

func (c *SnapshotCache) Current(context.Context) (Snapshot, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if !c.ready {
		if c.lastErr != nil {
			return Snapshot{}, c.lastErr
		}
		return Snapshot{}, ErrSnapshotUnavailable
	}
	return c.snapshot, nil
}

func (c *SnapshotCache) Run(ctx context.Context, interval time.Duration) {
	if interval <= 0 {
		interval = 5 * time.Second
	}
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			_ = c.Refresh(ctx)
		}
	}
}
