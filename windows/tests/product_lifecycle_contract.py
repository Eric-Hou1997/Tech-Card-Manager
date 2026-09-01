#!/usr/bin/env python3
"""Windows legacy product lifecycle and UX contract."""
from pathlib import Path
import hashlib
import struct

ROOT = Path(__file__).resolve().parents[1]
main = (ROOT / "main.go").read_text(encoding="utf-8")
platform = (ROOT / "platform_windows.go").read_text(encoding="utf-8")
service = (ROOT / "service_controller.go").read_text(encoding="utf-8")
tray = (ROOT / "tray_windows.go").read_text(encoding="utf-8")
engine = (ROOT / "engine" / "windows-engine.ps1").read_text(encoding="utf-8-sig")
card = (ROOT / "engine" / "technical-specs-card.js").read_text(encoding="utf-8")
web = (ROOT / "web" / "index.html").read_text(encoding="utf-8")
logo = (ROOT / "assets" / "imdb-app-icon.png").read_bytes()
EXPECTED_LOGO_SHA256 = "4abaf9c521b1422fb1a5de3f0175b28dc023896e4cd70f436b1f407a1444118f"

checks = {
    "single visual owner, no resident agent loop": (
        "func agentLoop" not in main
        and "platformStartAgent" not in main + platform
        and "ignored deprecated --agent launch" in main
    ),
    "login autostart is explicit and silent mode is login-only": (
        "--login-startup" in main + platform
        and "platformHideManagerWindowToTray" in main + tray
        and 'id="autoStart"' in web and 'id="silentStart"' in web
    ),
    "service state machine is explicit": all(x in service for x in [
        'serviceRunning', 'serviceStopping', 'serviceStopped',
        'serviceLegacyBlocked', 'serviceMigrating', 'serviceExiting',
    ]),
    "normal close confirms and shuts down": all(x in main + platform for x in [
        "platformConfirmExit", "services.shutdown()", "即将", "撤下技术规格卡片",
    ]),
    "stop cannot renew the lease after disabling it": (
        service.index("waiting for scheduler") < service.index("leaseErr := c.publishLease(false)")
    ),
    "owned PowerShell tree is cancelled": all(x in main for x in [
        'taskkill.exe', 'cmd.Process.Pid', '"/T"', '"/F"',
    ]),
    "owned Edge uses portable profile and is reaped": all(x in platform for x in [
        'edge-sessions', '--disable-background-mode', '--user-data-dir=',
        'CreateJobObjectW', 'AssignProcessToJobObject', 'TerminateJobObject',
        'platformStopAppWindowProcess',
    ]) and all(x in tray for x in ["GetWindowThreadProcessId", "platformOwnsAppWindowPID"]),
    "second launch restores one instance": all(x in tray for x in [
        "CreateMutexW", "ERROR_ALREADY_EXISTS", "requestExistingManagerRestore", "trayRestore",
    ]),
    "legacy detection is read-only until confirmation": all(x in service + platform + web for x in [
        "platformDetectLegacy", "legacy-cancel", "beginLegacyMigration",
        "取消，保持旧版不变", "再次点击启动服务时会重新检查",
    ]),
    "confirmed migration stops only revalidated legacy processes": all(x in platform for x in [
        "platformStopLegacyProcesses(report.Processes, w)",
        "strings.EqualFold(filepath.Clean(candidate.Path), filepath.Clean(process.Path))",
        "PID files can outlive their process and Windows may recycle the PID",
    ]),
    "unsafe legacy patch fails closed": (
        "UnsafePatch" in service
        and "无法确认所有权" in platform
        and "不会覆盖 index.html" in platform
    ),
    "card runtime lease controls visibility": all(x in service + platform + card + engine for x in [
        "technical-specs-runtime.json", "cardLeaseLifetime", "expires_at",
        "runtimeLeaseIsValid", "removeOldTechnicalCards", "$RuntimeStateFile",
        'st.Extra["card_runtime_valid"]',
    ]),
    "card lease cannot outlive a crash indefinitely": (
        "CARD_LEASE_POLL_MS" in card
        and 'response.headers.get("Date")' in card
        and "cachedRuntimeValidUntil" in card
        and "leaseInterval = setInterval" in card
        and "cardLeaseRenewInterval" in service
    ),
    "wide cards stay between one and two standard cards": all(x in card for x in [
        "min-width:min(var(--itm-standard-card-width)",
        "max-width:min(var(--itm-wide-card-max-width)",
        "standardWidth * 2",
    ]),
    "header matches product naming": all(x in web for x in [
        "<title>Tech Card Manager</title>", "v4.0.0",
        "Emby Server 技术规格卡片", 'src="../assets/imdb-app-icon.png"',
    ]) and "IMDb Tech Manager Windows" not in web,
    "formal display name is consistent across runtime surfaces": (
        "Tech-Card-Manager" not in main + platform + tray + engine + web
        and all(x in main + platform + tray + engine + web for x in [
            "Tech Card Manager", 'const winRunValueName = "Tech Card Manager"',
        ])
    ),
    "logo works in file preview and the packaged HTTP app": (
        'href="../assets/imdb-app-icon.png"' in web
        and '//go:embed web/index.html engine/windows-engine.ps1 engine/technical-specs-card.js assets/imdb-app-icon.png' in main
        and 'mux.HandleFunc("/assets/imdb-app-icon.png", serveAppIcon)' in main
        and 'assets.ReadFile("assets/imdb-app-icon.png")' in main
        and 'w.Header().Set("Content-Type", "image/png")' in main
        and logo.startswith(b"\x89PNG\r\n\x1a\n")
        and len(logo) >= 24
        and struct.unpack(">II", logo[16:24]) == (1024, 1024)
        and hashlib.sha256(logo).hexdigest() == EXPECTED_LOGO_SHA256
    ),
    "console has one service control": (
        web.count('id="serviceButton"') == 1
        and "Portable Manager" not in web
        and "登录后自动启动" not in web
        and "服务由当前可视化程序统一管理。" not in web
    ),
    "console visual hierarchy matches the accepted UI": all(x in web for x in [
        ".version{color:var(--muted);font-size:21px;font-weight:400",
        "h1{font-size:23px}.version{font-size:18px}",
        ".console{display:grid;grid-template-columns:minmax(320px,.72fr) minmax(0,1.28fr);gap:18px;align-items:stretch}",
        ".serviceCard{width:100%;min-width:300px;min-height:94px",
        ".serviceButton{flex:0 0 72px;width:72px;height:72px;min-width:72px",
        ".serviceButton.running{background:var(--imdb-yellow)",
        "Emby 卡片显示服务已启动",
        "上次启动时间：",
        ".consoleHint{background:var(--imdb-hint-bg)",
        "Windows 只读索引媒体库 NFO 的 <code>&lt;technicalspecs&gt;</code>，最小化后继续运行；关闭窗口将停止服务并撤下 Emby 技术规格卡片。",
    ]),
    "movie and tv retain independent state": all(x in web for x in [
        "catalogViewState", "movies:{query:", "tv:{query:", "switchCatalogSpace",
    ]),
    "invalid nfo remains visible": all(x in engine + web for x in [
        "InvalidNFO", "xmlErrorByPath", "解析异常", "索引尚不可用",
    ]),
    "windows remains nfo read only": (
        "Windows 只读索引媒体库 NFO" in web
        and "Save-Xml" not in engine
    ),
}

failed = [name for name, ok in checks.items() if not ok]
for name, ok in checks.items():
    print(("OK  " if ok else "FAIL ") + name)
if failed:
    raise SystemExit("Windows legacy product contract failed: " + ", ".join(failed))
print("OK Windows v4.0.0 product lifecycle, migration, lease, UI and NFO behavior")
