#!/usr/bin/env python3
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
main = (ROOT / 'main.go').read_text(encoding='utf-8')
platform = (ROOT / 'platform_windows.go').read_text(encoding='utf-8')
tray = (ROOT / 'tray_windows.go').read_text(encoding='utf-8')
engine_bytes = (ROOT / 'engine' / 'windows-engine.ps1').read_bytes()
engine = engine_bytes.decode('utf-8-sig')
web = (ROOT / 'web' / 'index.html').read_text(encoding='utf-8')

checks = {
    'manager version 4.0.1': 'const appVersion = "4.0.1"' in main,
    'PowerShell source has UTF-8 BOM': engine_bytes.startswith(b'\xef\xbb\xbf'),
    'installed PowerShell BOM is enforced': 'withUTF8BOM' in main,
    'configured roots are persisted': all(x in main for x in ['LibraryRoot', 'library_roots', 'roots_configured']),
    'root settings are validated': 'sanitizeLibraryRoots' in main,
    'native folder selection action': 'choose-library-root' in main and 'platformChooseFolder' in platform,
    'engine receives root config': '-RootConfigPath' in platform and 'RootConfigPath' in engine,
    'engine uses selected roots': 'Get-SelectedMediaLibraries' in engine,
    'Emby discovery remains available': 'discover-roots' in main and '-DiscoverOnly' in platform,
    'tray decodes LOWORD': 'trayNotificationCode' in tray and 'uint16(lParam & 0xffff)' in tray,
    'tray supports all restore events': all(x in tray for x in ['wmLButtonUp', 'wmLButtonDblCl', 'ninSelect', 'ninKeySelect']),
    'Chinese media root UI': all(x in web for x in ['媒体目录', '从 Emby 发现目录', '选择文件夹', '保存目录']),
    'portable lifecycle': '最小化后继续运行；关闭窗口将停止服务并撤下 Emby 技术规格卡片' in web and 'portableRootDir' in platform,
    'Windows remains read-only': 'Windows 只读索引媒体库 NFO' in web and 'Save-Xml' not in engine,
    'no AI producer': all(x not in main + web for x in ['Qwen', 'ai-generate', 'local-rebuild', 'AI Runtime']),
}

failed = [name for name, ok in checks.items() if not ok]
for name, ok in checks.items():
    print(('OK  ' if ok else 'FAIL ') + name)
if failed:
    raise SystemExit('Windows baseline contract failed: ' + ', '.join(failed))
