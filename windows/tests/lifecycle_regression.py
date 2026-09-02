#!/usr/bin/env python3
"""Windows legacy cross-version migration and owned-UI lifecycle regression contract."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
main = (ROOT / "main.go").read_text(encoding="utf-8")
platform = (ROOT / "platform_windows.go").read_text(encoding="utf-8")
service = (ROOT / "service_controller.go").read_text(encoding="utf-8")
service_test = (ROOT / "service_controller_windows_test.go").read_text(encoding="utf-8")
tray = (ROOT / "tray_windows.go").read_text(encoding="utf-8")
engine = (ROOT / "engine" / "windows-engine.ps1").read_text(encoding="utf-8-sig")
web = (ROOT / "web" / "index.html").read_text(encoding="utf-8")
confirm_start = main.index("confirmExit := func")
confirm_end = main.index("\n\tfor {", confirm_start)
confirm_exit = main[confirm_start:confirm_end]
confirmed_exit = confirm_exit[confirm_exit.index("platformHideManagerWindow()"):]

checks = {
    "manager version is 4.0.3": all(x in main + engine + web for x in [
        'const appVersion = "4.0.3"',
        "$ManagerVersion = '4.0.3'",
        "v4.0.3",
    ]),
    "cross-folder upgrade accepts only one exact owned block": all(x in engine for x in [
        "$strictOwnedPatch", "$insideScriptCount -eq 1", "$scriptCount -eq 1",
        "IsNullOrWhiteSpace($insideWithoutScript)", "StrictOwnedPatch = $strictOwnedPatch",
    ]),
    "ambiguous old patch still fails closed": all(x in engine for x in [
        "UNSAFE_WEB_PATCH_OWNERSHIP", "拒绝自动删除或覆盖",
    ]) and all(x in platform for x in [
        "scriptMatches", "strictOwned", "insideScripts", "withoutOwnedScript",
        "标记、脚本数量或块内容异常",
    ]),
    "verified owned block becomes the new immutable baseline": all(x in engine for x in [
        "if ($CleanResult.StrictOwnedPatch)",
        "Save-ImmutableBaseline -Context $Context -Snapshot $CleanResult.Snapshot",
    ]),
    "PowerShell failure is surfaced instead of exit status only": all(x in platform for x in [
        "compactPowerShellFailure", "io.MultiWriter", "details.String()",
        "Emby 网页卡片安装失败：%s",
    ]),
    "each UI launch has a unique owned Edge session": all(x in platform for x in [
        '"edge-sessions"', 'os.MkdirTemp(sessionsRoot, "session-")',
        '"--user-data-dir="+profile',
    ]),
    "crash-left Edge profiles are cleaned only after single-instance ownership": all(x in platform + main for x in [
        "cleanupStaleEdgeProfiles()", 'strings.HasPrefix(entry.Name(), "session-")',
        "platformAcquireManagerInstance()", "ensurePortableWorkspace()",
    ]) and main.index("platformAcquireManagerInstance()") < main.index("ensurePortableWorkspace()"),
    "Edge tree is placed in a kill-on-close Job": all(x in platform for x in [
        "CreateJobObjectW", "SetInformationJobObject", "AssignProcessToJobObject",
        "jobObjectLimitKillOnJobClose", "TerminateJobObject",
    ]),
    "Edge cannot spawn before Job ownership is established": all(x in platform for x in [
        "createSuspended", "NtResumeProcess", "resumeOwnedProcess",
        "cmd.SysProcAttr.CreationFlags |= createSuspended",
    ]),
    "launcher completion is not treated as UI completion": (
        "The launcher ending is not proof" in platform
        and "windowsHwnd" in platform
        and "windowPID" in platform
    ),
    "real HWND and process ownership are recorded": all(x in platform + tray for x in [
        "platformOwnsAppWindowPID", "platformRecordAppWindow",
        "GetWindowThreadProcessId", "platformOwnedAppWindowHWND", "isOwnedManagerWindow",
    ]),
    "window-close monitoring survives tray icon failure": (
        "Window lifecycle ownership is independent" in tray
        and tray.index("go watchManagerWindow()") < tray.index("procCreateWindow.Call")
        and "trayIconReady && isIconic(hwnd)" in tray
    ),
    "a window closed before first enumeration still triggers shutdown": all(x in platform + tray for x in [
        "QueryInformationJobObject", "ActiveProcesses",
        "hadOwnedSession && !platformAppWindowProcessesActive()",
    ]),
    "tray exit hides then verifies UI before server shutdown": (
        confirmed_exit.index("platformHideManagerWindow()") < confirmed_exit.index("services.shutdown()")
        < confirmed_exit.index("platformStopAppWindowProcess()") < confirmed_exit.index("\n\t\tshutdown()")
    ),
    "service shutdown failure blocks visual exit": all(x in confirm_exit for x in [
        'appendManagerLog("shutdown services: " + err.Error())',
        "platformShowShutdownError(err)", "restoreManagerWindow()", "return false",
    ]),
    "lease failures cannot report a false running or stopped state": all(x in service + main + web for x in [
        'serviceStopError     = "stop-error"',
        "无法建立 Emby 卡片运行许可", "撤卡状态写入失败",
        "Emby 卡片显示服务关闭异常", "未能确认 Emby 撤卡",
    ]) and all(x in service_test for x in [
        "TestInitialLeaseFailureDoesNotReportRunning",
        "TestStopLeaseFailureIsRetryableAndNeverReportsSuccess",
    ]),
    "localhost UI has bounded HTTP resources and request bodies": all(x in main for x in [
        "ReadHeaderTimeout", "ReadTimeout", "WriteTimeout", "IdleTimeout",
        "MaxHeaderBytes", "http.MaxBytesReader",
    ]),
    "generated tag color uses authoritative embedded ownership only": all(x in engine + web for x in [
        "$ParserCacheVersion = 'tech-card-cache-1'", "./generatedtags", "./manualtags",
        "Get-CanonicalTagValue", "ownership = 'external'", "ownership = 'generated'",
        "tag.ownership==='generated'", "managerGenerated",
        "本程序生成（来自 NFO ownership 清单）",
    ]) and "Never guess ownership from a tag's wording" in engine,
    "failed UI cleanup keeps Manager alive for retry": all(x in main + platform for x in [
        "platformShowShutdownError", "restoreManagerWindow()",
        "程序将保持运行以便重试", "return false",
    ]),
    "second launch uses exact tray-host IPC": all(x in tray for x in [
        "FindWindowW", "requestExistingManagerRestore", "trayRestore",
        "TechCardManagerTrayHost400", "Tech Card Manager Tray Host",
    ]),
    "tray restore cannot create a second window while a session exists": (
        "!platformAppWindowSessionActive()" in tray
    ),
    "blocking administrator work has a bounded exit wait": all(x in main + service for x in [
        "waitForBlockingJob(30 * time.Second)", "errMaintenanceExitTimeout",
        "time.NewTimer(timeout)",
    ]),
    "legacy dialog distinguishes components and states the exact action": all(x in web for x in [
        "检测到旧版 Card 软件组件", "旧程序、后台组件或网页卡片",
        "迁移所列组件并继续",
    ]) and 'CommandLine string `json:"-"`' in service,
}

failed = [name for name, ok in checks.items() if not ok]
for name, ok in checks.items():
    print(("OK  " if ok else "FAIL ") + name)
if failed:
    raise SystemExit("Windows legacy lifecycle regression failed: " + ", ".join(failed))
print("OK Windows legacy cross-version migration and owned-UI lifecycle regression contract")
