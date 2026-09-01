package main

import (
	"context"
	"errors"
	"fmt"
	"io"
	"sync"
	"time"
)

const (
	serviceStarting      = "starting"
	serviceRunning       = "running"
	serviceStopping      = "stopping"
	serviceStopped       = "stopped"
	serviceLegacyBlocked = "legacy-blocked"
	serviceMigrating     = "migrating"
	serviceError         = "error"
	serviceStopError     = "stop-error"
	serviceExiting       = "exiting"

	cardLeaseRenewInterval = 2 * time.Second
	cardLeaseLifetime      = 8 * time.Second
)

var errLegacyMigrationRequired = errors.New("检测到旧版组件，需要用户确认迁移")
var writeCardRuntime = platformWriteCardRuntime
var loadServiceSettings = loadSettings
var serviceIndexesReady = derivedIndexesValidForSettings

type LegacyReport struct {
	Required      bool            `json:"required"`
	AgentRunning  bool            `json:"agent_running"`
	AutoStart     bool            `json:"auto_start"`
	ScheduledTask bool            `json:"scheduled_task"`
	Artifacts     bool            `json:"artifacts"`
	WebPatch      bool            `json:"web_patch"`
	UnsafePatch   bool            `json:"unsafe_patch"`
	Items         []string        `json:"items,omitempty"`
	Processes     []LegacyProcess `json:"processes,omitempty"`
}

type LegacyProcess struct {
	PID         int    `json:"pid"`
	Path        string `json:"path"`
	CommandLine string `json:"-"`
}

type ServiceSnapshot struct {
	State               string       `json:"state"`
	Running             bool         `json:"running"`
	Message             string       `json:"message,omitempty"`
	LastStartedAt       string       `json:"last_started_at,omitempty"`
	Legacy              LegacyReport `json:"legacy"`
	LegacyPromptPending bool         `json:"legacy_prompt_pending"`
	CardLeaseActive     bool         `json:"card_lease_active"`
}

type CardRuntimeState struct {
	Version        int    `json:"version"`
	ManagerVersion string `json:"manager_version"`
	WebCardVersion string `json:"web_card_version"`
	SessionID      string `json:"session_id"`
	Sequence       uint64 `json:"sequence"`
	Enabled        bool   `json:"enabled"`
	UpdatedAt      string `json:"updated_at"`
	ExpiresAt      string `json:"expires_at"`
}

type serviceController struct {
	mu                  sync.Mutex
	state               string
	message             string
	legacy              LegacyReport
	legacyPromptPending bool
	cardLeaseActive     bool
	lastStartedAt       string
	ctx                 context.Context
	cancel              context.CancelFunc
	done                chan struct{}
	sessionID           string
	leaseSequence       uint64
}

var services = serviceController{state: serviceStopped, message: "服务已停止"}

func (c *serviceController) initialize() {
	c.mu.Lock()
	c.state = serviceStarting
	c.message = "正在检查运行环境…"
	c.mu.Unlock()

	report := platformDetectLegacy()
	if report.Required {
		c.mu.Lock()
		c.state = serviceLegacyBlocked
		c.message = "检测到旧版组件；新版服务尚未启动"
		c.legacy = report
		c.legacyPromptPending = true
		c.mu.Unlock()
		return
	}

	if err := c.startWithoutLegacyCheck(); err != nil {
		c.setError(err)
	}
}

func (c *serviceController) snapshot() ServiceSnapshot {
	c.mu.Lock()
	defer c.mu.Unlock()
	return ServiceSnapshot{
		State:               c.state,
		Running:             c.state == serviceRunning,
		Message:             c.message,
		LastStartedAt:       c.lastStartedAt,
		Legacy:              c.legacy,
		LegacyPromptPending: c.legacyPromptPending,
		CardLeaseActive:     c.cardLeaseActive,
	}
}

func (c *serviceController) requestStart() error {
	c.mu.Lock()
	if c.state == serviceRunning || c.state == serviceStarting {
		c.mu.Unlock()
		return nil
	}
	if c.state == serviceStopping || c.state == serviceMigrating || c.state == serviceExiting {
		state := c.state
		c.mu.Unlock()
		return fmt.Errorf("服务当前处于 %s 状态，请稍候", state)
	}
	c.state = serviceStarting
	c.message = "正在重新检查旧版组件…"
	c.legacyPromptPending = false
	c.mu.Unlock()

	report := platformDetectLegacy()
	if report.Required {
		c.mu.Lock()
		c.state = serviceLegacyBlocked
		c.message = "检测到旧版组件；新版服务尚未启动"
		c.legacy = report
		c.legacyPromptPending = true
		c.mu.Unlock()
		return errLegacyMigrationRequired
	}

	return c.startWithoutLegacyCheck()
}

func (c *serviceController) startWithoutLegacyCheck() error {
	settings := loadServiceSettings()
	if !settings.RootsConfigured {
		c.mu.Lock()
		c.state = serviceStopped
		c.message = "服务已停止；请先确认媒体目录"
		c.cardLeaseActive = false
		c.mu.Unlock()
		_ = c.publishLease(false)
		return nil
	}

	c.mu.Lock()
	if c.state == serviceRunning {
		c.mu.Unlock()
		return nil
	}
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	c.ctx = ctx
	c.cancel = cancel
	c.done = done
	c.sessionID = newSessionToken()
	c.leaseSequence = 0
	indexesReady := serviceIndexesReady(settings)
	if indexesReady {
		c.state = serviceRunning
		c.message = "服务运行中"
	} else {
		c.state = serviceStarting
		c.message = "正在恢复媒体目录并建立卡片索引…"
	}
	c.legacy = LegacyReport{}
	c.legacyPromptPending = false
	c.cardLeaseActive = false
	c.mu.Unlock()

	if indexesReady {
		if err := c.publishLease(true); err != nil {
			appendManagerLog("card lease start: " + err.Error())
			cancel()
			close(done)
			c.mu.Lock()
			c.ctx = nil
			c.cancel = nil
			c.done = nil
			c.state = serviceError
			c.message = "服务启动失败：无法建立 Emby 卡片运行许可：" + err.Error()
			c.cardLeaseActive = false
			c.mu.Unlock()
			return fmt.Errorf("无法建立 Emby 卡片运行许可：%w", err)
		}
		c.recordSuccessfulStart()
	}
	go c.run(ctx, done, indexesReady)
	return nil
}

func (c *serviceController) failInitialIndex(err error) {
	appendManagerLog("initial card index: " + err.Error())
	c.mu.Lock()
	if c.state != serviceStopping && c.state != serviceExiting {
		c.state = serviceError
		c.message = "服务启动失败：卡片索引没有建立成功：" + err.Error()
		c.cardLeaseActive = false
	}
	c.mu.Unlock()
}

func (c *serviceController) run(ctx context.Context, done chan struct{}, indexesReady bool) {
	defer close(done)

	if !indexesReady {
		initialDone := make(chan error, 1)
		err := startManagedJob(ctx, "auto", managedJobOptions{
			message: "正在建立卡片索引",
			run: func(jobCtx context.Context, w io.Writer) error {
				return performActionWithWriter(jobCtx, "auto", "", w)
			},
			after: func(runErr error) { initialDone <- runErr },
		})
		if err != nil {
			c.failInitialIndex(err)
			return
		}
		select {
		case <-ctx.Done():
			return
		case err := <-initialDone:
			if err != nil {
				c.failInitialIndex(err)
				return
			}
		}
		if !derivedIndexesValid() {
			c.failInitialIndex(fmt.Errorf("索引文件未通过版本、目录与一致性复核"))
			return
		}
		c.mu.Lock()
		if c.state != serviceStarting {
			c.mu.Unlock()
			return
		}
		c.state = serviceRunning
		c.message = "服务运行中"
		c.mu.Unlock()
		if err := c.publishLease(true); err != nil {
			c.failInitialIndex(fmt.Errorf("无法建立 Emby 卡片运行许可：%w", err))
			return
		}
		c.recordSuccessfulStart()
	} else {
		_ = startJobWithParent(ctx, "auto", "")
	}

	leaseTicker := time.NewTicker(cardLeaseRenewInterval)
	defer leaseTicker.Stop()

	nextScan := time.NewTimer(serviceScanInterval())
	defer nextScan.Stop()

	for {
		select {
		case <-ctx.Done():
			return
		case <-leaseTicker.C:
			if err := c.publishLease(true); err != nil {
				appendManagerLog("card lease renew: " + err.Error())
			}
		case <-nextScan.C:
			if loadSettings().RootsConfigured {
				_ = startJobWithParent(ctx, "auto", "")
			}
			nextScan.Reset(serviceScanInterval())
		}
	}
}

func (c *serviceController) recordSuccessfulStart() {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.state == serviceRunning {
		c.lastStartedAt = time.Now().Format(time.RFC3339)
	}
}

func serviceScanInterval() time.Duration {
	seconds := loadSettings().IntervalSeconds
	if seconds < 30 {
		seconds = 60
	}
	return time.Duration(seconds) * time.Second
}

func (c *serviceController) publishLease(enabled bool) error {
	c.mu.Lock()
	c.leaseSequence++
	sequence := c.leaseSequence
	sessionID := c.sessionID
	c.mu.Unlock()

	now := time.Now().UTC()
	expiresAt := now
	if enabled {
		expiresAt = now.Add(cardLeaseLifetime)
	}
	err := writeCardRuntime(CardRuntimeState{
		Version:        1,
		ManagerVersion: appVersion,
		WebCardVersion: expectedWebCardVersion,
		SessionID:      sessionID,
		Sequence:       sequence,
		Enabled:        enabled,
		UpdatedAt:      now.Format(time.RFC3339Nano),
		ExpiresAt:      expiresAt.Format(time.RFC3339Nano),
	})

	c.mu.Lock()
	c.cardLeaseActive = enabled && err == nil
	if enabled && c.state == serviceRunning {
		if err == nil {
			c.message = "服务运行中"
		} else {
			c.message = "服务运行中，但 Emby 卡片运行许可不可用：" + err.Error()
		}
	}
	c.mu.Unlock()
	return err
}

func (c *serviceController) stop(message string) error {
	return c.stopInternal(message, true)
}

func (c *serviceController) stopForMaintenance(message string) error {
	return c.stopInternal(message, false)
}

func (c *serviceController) stopInternal(message string, cancelJob bool) error {
	c.mu.Lock()
	if c.state == serviceStopped || c.state == serviceLegacyBlocked {
		c.message = message
		c.cardLeaseActive = false
		c.mu.Unlock()
		if cancelJob {
			cancelActiveJob()
		}
		return nil
	}
	if c.state == serviceStopping || c.state == serviceExiting {
		done := c.done
		c.mu.Unlock()
		if done != nil {
			select {
			case <-done:
			case <-time.After(8 * time.Second):
			}
		}
		if cancelJob {
			cancelActiveJob()
		}
		return nil
	}
	c.state = serviceStopping
	c.message = "正在停止服务并撤下 Emby 技术规格卡片…"
	cancel := c.cancel
	done := c.done
	c.mu.Unlock()

	if cancel != nil {
		cancel()
	}
	// Stop the scheduler before publishing disabled. Otherwise a lease ticker
	// already selected by the scheduler could renew enabled=true after stop had
	// written enabled=false.
	if done != nil {
		select {
		case <-done:
		case <-time.After(8 * time.Second):
			appendManagerLog("service stop timed out waiting for scheduler")
		}
	}
	if cancelJob {
		cancelActiveJob()
	}
	leaseErr := c.publishLease(false)

	c.mu.Lock()
	c.ctx = nil
	c.cancel = nil
	c.done = nil
	c.cardLeaseActive = false
	if leaseErr != nil {
		c.state = serviceStopError
		c.message = "服务调度已停止，但撤卡状态写入失败；请重试停止：" + leaseErr.Error()
	} else {
		c.state = serviceStopped
		c.message = message
	}
	c.mu.Unlock()
	return leaseErr
}

func (c *serviceController) shutdown() error {
	if err := waitForBlockingJob(30 * time.Second); err != nil {
		return err
	}
	c.mu.Lock()
	legacyBlocked := c.state == serviceLegacyBlocked
	c.mu.Unlock()
	if legacyBlocked {
		// A user can still run an explicit administrator maintenance action while
		// the service is legacy-blocked. Wait for that owned transaction before the
		// visual Manager exits so its elevated helper cannot be orphaned.
		cancelActiveJob()
		c.mu.Lock()
		c.state = serviceExiting
		c.message = "新版服务未启动；正在退出"
		c.mu.Unlock()
		return nil
	}
	err := c.stop("服务已停止")
	c.mu.Lock()
	if err != nil {
		c.state = serviceStopError
		c.message = "服务调度已停止，但未能确认 Emby 撤卡；退出已暂停：" + err.Error()
	} else {
		c.state = serviceExiting
		c.message = "全部服务已停止；正在退出"
	}
	c.mu.Unlock()
	return err
}

func (c *serviceController) cancelLegacyMigration() {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.state = serviceLegacyBlocked
	c.message = "服务已停止；旧版组件保持不变"
	c.legacyPromptPending = false
	c.cardLeaseActive = false
}

func (c *serviceController) beginLegacyMigration() error {
	c.mu.Lock()
	if c.state != serviceLegacyBlocked {
		c.mu.Unlock()
		return fmt.Errorf("当前没有等待迁移的旧版组件")
	}
	c.state = serviceMigrating
	c.message = "正在申请管理员权限以迁移旧版组件…"
	c.legacyPromptPending = false
	c.mu.Unlock()

	_, err := startPrivilegedJob("migrate-legacy", nil, func(runErr error) {
		if runErr != nil {
			c.mu.Lock()
			c.state = serviceLegacyBlocked
			c.message = "旧版迁移未完成：" + runErr.Error()
			c.legacy = platformDetectLegacy()
			c.legacyPromptPending = true
			c.cardLeaseActive = false
			c.mu.Unlock()
			return
		}

		c.mu.Lock()
		stillMigrating := c.state == serviceMigrating
		c.mu.Unlock()
		if !stillMigrating {
			return
		}
		if err := c.startWithoutLegacyCheck(); err != nil {
			c.setError(err)
		}
	})
	if err != nil {
		c.mu.Lock()
		c.state = serviceLegacyBlocked
		c.message = "旧版迁移没有开始：" + err.Error()
		c.legacyPromptPending = true
		c.mu.Unlock()
		return err
	}
	return nil
}

func (c *serviceController) setError(err error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.state = serviceError
	c.message = err.Error()
	c.cardLeaseActive = false
}

func serviceIsRunning() bool {
	return services.snapshot().Running
}
