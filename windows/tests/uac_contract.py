#!/usr/bin/env python3
"""Behavioral source contract for the legacy administrator-operation lifecycle."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
main = (ROOT / "main.go").read_text(encoding="utf-8")
platform = (ROOT / "platform_windows.go").read_text(encoding="utf-8")
service = (ROOT / "service_controller.go").read_text(encoding="utf-8")
web = (ROOT / "web" / "index.html").read_text(encoding="utf-8")

checks = {
    "elevation is detected instead of guessed": all(
        value in platform
        for value in ["platformProcessIsElevated", "OpenProcessToken", "TokenElevation"]
    ),
    "runas returns an owned process handle": all(
        value in platform
        for value in ["ShellExecuteExW", "SEE_MASK_NOCLOSEPROCESS", "HProcess"]
    ) and 'NewProc("ShellExecuteW")' not in platform,
    "uac cancellation is explicit": all(
        value in platform
        for value in ["errno == 1223", "用户取消了管理员权限请求，操作未执行"]
    ),
    "each request has isolated result files": all(
        value in platform
        for value in [
            "newElevatedRequestID",
            'requestID+".ready.json"',
            'requestID+".permit.json"',
            'requestID+".result.json"',
        ]
    ) and '"elevated-result.json"' not in platform,
    "helper cannot mutate before parent permission": (
        main.index("waitForElevatedPermit") < main.index("performImmediateAction")
        and platform.index("waitForElevatedReady") < platform.index("beforeRun")
        and platform.index("beforeRun") < platform.index("writeElevatedPermit(requestID, action, true")
        and '" --manager-pid "' in platform
        and "主程序已经退出，管理员操作没有执行" in platform
    ),
    "result is correlated and postverified": all(
        value in main + platform
        for value in [
            "result.RequestID != requestID",
            "result.Action != action",
            "verifyPrivilegedAction",
            "网页集成文件没有通过完整性复核",
        ]
    ),
    "failed elevation never falls through to normal execution": (
        "if isPrivilegedAction(req.Action)" in main
        and 'writeJSONStatus(w, http.StatusConflict' in main
        and "startPrivilegedJob(req.Action" in main
        and 'if err := requestElevatedAction(req.Action)' not in main
    ),
    "disable waits for authorization before stopping service": (
        'beforeRun = func() error' in main
        and 'services.stopForMaintenance' in main
        and 'platformRunElevatedAction(ctx, action, beforeRun, w)' in main
        and "stopInternal(message, false)" in service
    ),
    "elevated transactions block orphaning on exit": all(
        value in main + platform
        for value in [
            "BlocksExit",
            "<-done",
            "管理员维护事务正在执行",
            "不会把提权进程留在后台",
        ]
    ),
    "ui wording is conditional and completion is visible": all(
        value in main + web
        for value in [
            "如 Windows 需要确认，将显示 UAC 窗口",
            "当前程序已具备管理员权限",
            "j.needs_admin",
            "已完成并通过结果复核",
        ]
    ) and "已请求管理员权限，请确认 UAC" not in main + web,
}

failed = [name for name, ok in checks.items() if not ok]
for name, ok in checks.items():
    print(("OK  " if ok else "FAIL ") + name)
if failed:
    raise SystemExit("Windows legacy UAC contract failed: " + ", ".join(failed))
print("OK Windows legacy administrator-operation lifecycle contract")
