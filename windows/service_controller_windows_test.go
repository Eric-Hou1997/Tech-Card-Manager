//go:build windows

package main

import (
	"errors"
	"testing"
	"time"
)

func TestSuccessfulStartTimestampIsExposedAndRetained(t *testing.T) {
	c := serviceController{state: serviceRunning, message: "服务运行中"}
	c.recordSuccessfulStart()
	started := c.snapshot().LastStartedAt
	if started == "" {
		t.Fatal("successful start did not expose last_started_at")
	}
	if _, err := time.Parse(time.RFC3339, started); err != nil {
		t.Fatalf("last_started_at is not RFC3339: %q: %v", started, err)
	}
	c.state = serviceStopped
	if got := c.snapshot().LastStartedAt; got != started {
		t.Fatalf("stopping erased the last successful start: got %q want %q", got, started)
	}
}

func TestStoppedServiceCannotInventSuccessfulStartTimestamp(t *testing.T) {
	c := serviceController{state: serviceStopped, message: "服务已停止"}
	c.recordSuccessfulStart()
	if got := c.snapshot().LastStartedAt; got != "" {
		t.Fatalf("stopped service invented last_started_at: %q", got)
	}
}

func TestInitialLeaseFailureDoesNotReportRunning(t *testing.T) {
	original := writeCardRuntime
	originalSettings := loadServiceSettings
	originalIndexesReady := serviceIndexesReady
	writeCardRuntime = func(CardRuntimeState) error { return errors.New("access denied") }
	loadServiceSettings = func() Settings {
		return Settings{RootsConfigured: true, LibraryRoots: []LibraryRoot{{Path: `D:\Movies`, Enabled: true}}}
	}
	serviceIndexesReady = func(Settings) bool { return true }
	defer func() {
		writeCardRuntime = original
		loadServiceSettings = originalSettings
		serviceIndexesReady = originalIndexesReady
	}()

	c := serviceController{state: serviceStopped, message: "服务已停止"}
	if err := c.startWithoutLegacyCheck(); err == nil {
		t.Fatal("expected initial lease error")
	}
	snapshot := c.snapshot()
	if snapshot.State != serviceError || snapshot.Running || snapshot.CardLeaseActive {
		t.Fatalf("initial lease failure was reported as healthy: %+v", snapshot)
	}
}

func TestStopLeaseFailureIsRetryableAndNeverReportsSuccess(t *testing.T) {
	original := writeCardRuntime
	fail := true
	writeCardRuntime = func(CardRuntimeState) error {
		if fail {
			return errors.New("access denied")
		}
		return nil
	}
	defer func() { writeCardRuntime = original }()

	done := make(chan struct{})
	close(done)
	c := serviceController{state: serviceRunning, done: done, sessionID: "test"}
	if err := c.stop("服务已停止"); err == nil {
		t.Fatal("expected stop lease error")
	}
	if snapshot := c.snapshot(); snapshot.State != serviceStopError || snapshot.Running {
		t.Fatalf("failed stop did not enter retryable stop-error: %+v", snapshot)
	}

	fail = false
	if err := c.stop("服务已停止"); err != nil {
		t.Fatalf("retry stop failed: %v", err)
	}
	if snapshot := c.snapshot(); snapshot.State != serviceStopped || snapshot.Running {
		t.Fatalf("successful retry did not stop cleanly: %+v", snapshot)
	}
}
