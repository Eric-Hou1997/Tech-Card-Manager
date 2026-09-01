from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
main=(ROOT/'main.go').read_text(encoding='utf-8')
web=(ROOT/'web/index.html').read_text(encoding='utf-8')
win=(ROOT/'engine/windows-engine.ps1').read_text(encoding='utf-8-sig')
card=(ROOT/'engine/technical-specs-card.js').read_text(encoding='utf-8')
platform=(ROOT/'platform_windows.go').read_text(encoding='utf-8')
tray=(ROOT/'tray_windows.go').read_text(encoding='utf-8')
checks={
 'manager version 4.0.0':'const appVersion = "4.0.0"' in main,
 'windows-only embed':'engine/mac-engine.py' not in main and 'engine/windows-engine.ps1' in main,
 'no AI runtime UI':all(x not in web for x in ['AI Runtime','Qwen','AI 智能生成 Tag','本地自动生成 Tag','System Prompt','ai-generate','local-rebuild']),
 'no AI runtime code':all(x not in main for x in ['AIConfig','defaultAIPrompt','/api/ai-config','ai-generate','local-rebuild']),
 'no imdb producer actions':all(x not in main for x in ['"backfill"','"reconcile"','"refresh"','"test-imdb"']),
 'card v4.0.0':'const WEB_CARD_VERSION = "4.0.0"' in card,
 'movie series eligible':'CARD_ELIGIBLE_TYPES = new Set(["Movie", "Series"])' in card,
 'episode season suppressed':'CARD_SUPPRESSED_TYPES = new Set(["Episode", "Season"])' in card,
 'iso native hierarchy':'getOrCreateNativeTarget(detailRoot)' in card and 'createNativeMediaHost' in card,
 'spa watchdog':'stale-card-watchdog' in card and 'item-watchdog' in card and 'misplaced-card-watchdog' not in card,
 'windows card version':"$ExpectedWebCardVersion = '4.0.0'" in win,
 'no historical js overwrite':'$JsBase64' not in win and 'FromBase64String' not in win,
 'episode and season excluded from public index':"([string]$obj.type) -eq 'Episode' -or ([string]$obj.type) -eq 'Season'" in win and 'episodeSpecsExcludedFromWeb' in win,
 'read-only positioning':'technicalspecs' in win.lower(),
 'tray minimize uses native hide':'Shell_NotifyIconW' in tray and 'procIsIconic' in tray and 'swHide' in tray,
 'tray restores app':'swRestore' in tray and 'SetForegroundWindow' in tray and '打开 Tech Card Manager' in tray,
 'tray has TCM icon':'//go:embed assets/tcm.ico' in tray,
 'window close is owned lifecycle':'WindowClosed' in main and 'platformConfirmExit' in main and 'services.shutdown()' in main,
 'login autostart and silent tray mode':'--login-startup' in platform and 'platformHideManagerWindowToTray' in tray and 'id="silentStart"' in web,
 'official square logo':'alt="Tech Card Manager"' in web and '/assets/TCM_logo_letter_only.png' in web,
 'repair web only switch':'[switch]$RepairWebOnly' in win,
 'repair web only action':'-RepairWebOnly' in platform,
 'startup never rewrites web':'platformRunEngine(ctx, "repair-web"' not in main and 'services.initialize()' in main,
 'agent index never repairs web':'Ensure-WebPatch' not in win and 'if ($IndexOnly)' in win and '[void](Update-TechIndex -SkipIfBusy)' in win,
 'diagnostic version dynamic':'$ExpectedWebCardVersion' in win,
 'web health surfaced':'web_healthy' in platform and 'web_index_injected' in platform and 'web_js_matches' in platform,
 'series eligible':'CARD_ELIGIBLE_TYPES' in card and 'Series' in card,
 'no strict root gate':'rootItemIdMatchScore' not in card and 'visibleRectArea' not in card,

}
for k,v in checks.items(): print(('OK  ' if v else 'FAIL')+k)
failed=[k for k,v in checks.items() if not v]
if failed: raise SystemExit('failed: '+', '.join(failed))
