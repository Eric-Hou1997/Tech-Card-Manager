//go:build windows

package main

import (
	"strings"
	"testing"
)

func TestTrayNotificationVersion4PackedLParam(t *testing.T) {
	const iconID = 7
	events := []uint16{wmLButtonUp, wmLButtonDblCl, ninSelect, ninKeySelect}
	for _, event := range events {
		packed := uintptr(uint32(iconID)<<16 | uint32(event))
		if got := trayNotificationCode(packed); got != event {
			t.Fatalf("notification code: got %#x want %#x", got, event)
		}
		if got := trayIconID(packed); got != iconID {
			t.Fatalf("icon id: got %d want %d", got, iconID)
		}
		if !shouldRestoreTrayEvent(event) {
			t.Fatalf("event %#x should restore the Manager window", event)
		}
	}
}

func TestTrayRightClickDoesNotRestore(t *testing.T) {
	if shouldRestoreTrayEvent(wmRButtonUp) {
		t.Fatal("right click must open the menu instead of restoring directly")
	}
}

func TestCompactPowerShellFailureKeepsActionableReason(t *testing.T) {
	output := "事务化安装/修复 Emby 技术规格网页卡片…\r\n" +
		"windows-engine.ps1 : NO_TRUSTED_BASELINE：没有可靠原版备份。\r\n" +
		"    + CategoryInfo          : OperationStopped\r\n" +
		"    + FullyQualifiedErrorId : RuntimeException\r\n"
	got := compactPowerShellFailure(output)
	if !strings.Contains(got, "NO_TRUSTED_BASELINE") {
		t.Fatalf("actionable PowerShell reason was lost: %q", got)
	}
	if strings.Contains(got, "FullyQualifiedErrorId") {
		t.Fatalf("PowerShell boilerplate leaked into user message: %q", got)
	}
}
