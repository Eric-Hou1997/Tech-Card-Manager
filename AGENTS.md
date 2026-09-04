# Tech Card Manager repository instructions

These instructions are part of the public repository. Apply them to every change in this repository, including changes made in a fork. A fork does not imply authority to publish an official upstream release.

## Maintainer shorthand

- In maintainer conversations only, `card软件` means the Tech Card Manager product in this repository, and `tech软件` means the separate IMDb-Tech-Manager product.
- These shorthand terms are coordination language, not product branding. Never place `card软件` or `tech软件` in user-facing UI, logs, packages, release names, or marketing copy.

## Repository identity and source of truth

- The formal product name is `Tech Card Manager`. Use it in current UI text, window/tray titles, services, logs, documentation, and other user-visible copy.
- The repository directory and hosting-site slug are `Tech-Card-Manager`. Machine-oriented artifact filenames may use this hyphenated slug, but it is not the display name.
- Product source starts at `v4.0.0`; do not import earlier product source, release artifacts, changelogs, tags, or version labels. Documentation-only GitHub history may be retained when synchronizing the public repository, but it must not be presented as earlier product source history.
- The authoritative development line is `main`. Follow the maintainer's branch and review workflow, and never rewrite published history without explicit authorization.
- The repository is organized by product, not permanently by operating system. The currently supported implementation is the Windows x64 application under `windows/`. A future macOS port belongs here only when explicitly scoped, and it must remain the same read-only Card Manager product.
- Do not copy IMDb scraping, NFO editing, AI/rule tag production, batch ownership management, or the `IMDb-Tech-Manager` product identity into a platform port of this repository.
- Platform-specific files or build stubs are not proof that a platform is supported. Support requires a complete product flow, packaging rules, tests, and real-platform acceptance.
- Inspect the current source, root README, release-note sources, tests, and embedded assets before editing. Extend the existing Go Core, local Web UI, PowerShell engine, tray, and browser integration architecture; do not replace the desktop stack or rewrite the product from scratch.
- Tests are evidence, not a complete behavior specification. Add characterization coverage before changing NFO parsing, legacy detection, Web Card patching, task scope, persistence, elevation, or lifecycle ownership.

## Repository and contribution hygiene

- 当用户找到一个 bug 或提出了一个很棒的想法时，请先回应：牛逼
- Keep product code, documentation, filenames, UI text, package metadata, and artifacts free of prerelease branding and pre-repository product version labels. Do not confuse compatibility identifiers with current branding.
- Do not commit local configuration, credentials, tokens, caches, browser profiles, logs, generated binaries, packaged executables, backups copied from a user's Emby installation, or release archives.
- The repository is licensed under Apache License 2.0. Retain `LICENSE` and `NOTICE` in redistributions, keep author attribution as `侯雁泽`, and do not alter the license, trademark policy, or asset-redistribution claims without an explicit maintainer decision.
- Treat `packaging/` as release-input source and `tools/build-release.sh` as a release recipe. Their presence does not mean that a package has been built, validated, tagged, or published.
- Do not claim that a source checkout, cross-build, fixture, mock, or static contract proves real Windows, PowerShell, UAC, tray, browser, or Emby DOM behavior.
- Preserve unrelated contributor changes. Do not perform broad cleanup, global replacement, history rewriting, dependency upgrades, or architecture migrations unless they are in the reviewed task scope.
- Do not assume access to a maintainer's local folders, private issue archives, previous repositories, credentials, Emby server, or release systems. The public repository must remain understandable and testable on its own.

## Product responsibilities and read-only boundary

- Tech Card Manager is a read-only Emby NFO index and Web Card manager on every platform.
- It may read configured Movie and TV library roots, build its own derived index, serve card assets, and transactionally install, upgrade, or remove the Web Card integration in Emby's web files.
- It must never scrape IMDb, modify media NFO files, generate or delete tags, run AI/Qwen/Prompt/Token workflows, edit Technical Specs, or migrate NFO ownership.
- Movie and TV are separate spaces with independent roots, selection, filters, refresh scopes, and visible state. “Refresh current library” must not silently scan the other space; full-library work is never the default.
- NFO ownership metadata may be displayed as read-only evidence. Never infer ownership from tag text and never rewrite the metadata.
- Errors identify the affected title, year, IMDb ID, full NFO path, media kind, and task when those fields are available, and UI errors must link back to the indexed item.

## Compatibility identifiers are data, not branding

- Existing `IMDbTechManager WebPatch` markers, related mutex/lock identifiers, the old Windows Run value, legacy executable detection, and ownership value `IMDb Tech Manager` are compatibility and provenance evidence. They intentionally identify artifacts created outside the current product identity.
- Do not globally replace those identifiers with `Tech Card Manager`, and do not present them as the current product name. Any change requires an explicit migration design, backward-compatible detection, rollback, and regression coverage against existing installations and NFO fixtures.
- Legacy process or component detection must use authoritative path, command-line, marker, manifest, and/or ownership evidence as appropriate. A process name, window title, stale PID, or text resemblance alone is never sufficient.
- Unknown or conflicting legacy ownership remains unclaimed. Cleanup must stop safely rather than delete, terminate, or overwrite an uncertain target.

## Web integration safety and lifecycle

- Media NFO files are read-only. Preserve their bytes and timestamps; index generation must not “normalize,” repair, or rewrite them.
- Emby Web Card installation or removal must validate the exact target, establish and verify a recoverable external backup, build a complete candidate, preserve required BOM/newline behavior, and commit transactionally. Partial or unverifiable work must roll back and report failure.
- Old-component migration, process termination, patch replacement, and destructive maintenance require a visible itemized plan and explicit user confirmation. Revalidate every target immediately before mutation and verify the postcondition afterward.
- Give every window, tray icon, browser profile, child/elevated process, timer, observer, job, port, lease, lock, temporary file, backup, and service one explicit owner with idempotent creation and synchronously verified shutdown.
- Second launch, repeated clicks, restore/minimize, upgrade, cancel, service stop, tray exit, and application exit must not create duplicate UI/resources or leave services, patches, locks, or processes in an ambiguous state.

## Product maturity

- A feature is complete only when its real chain is proven: configured roots -> parsed read-only NFO -> derived index -> served assets -> browser/client load -> visible cards -> stop and cleanup behavior.
- Status must distinguish requested, running, disk-ready, service-served, client-loaded, rendered, stopped, failed, and unverified states. Never report success from an intermediate prerequisite.
- Core setup, permissions/elevation, progress, cancellation, recovery, and actionable failures must be visible in normal product flows. Long work must provide honest phase/progress feedback.
- Preserve structured failure reasons and surface concise actionable messages; do not swallow errors that affect visible behavior.
- Proactively audit normal, first-run, empty, slow, cancelled, minimized/restored, repeated-click, second-launch, upgrade, rollback, partial-failure, crash-recovery, offline, permission-denied, and exit paths.
- Add behavioral and state-transition regression coverage for every defect and adjacent negative path. Source-string assertions alone are not sufficient proof.

## Localization, Web Card, and upgrade compatibility

- Keep Simplified Chinese (`zh-CN`), Traditional Chinese (`zh-Hant`), and English (`en-US`) built into the application. French, Russian, Japanese, Spanish, and Thai are external presentation-only language packs unless the maintainer changes the supported-language registry.
- Ship external packs as assets of an application GitHub Release, not from a separate language repository. The application catalog must bind each app version to one exact descriptor for each locale. An unchanged pack may keep its earlier `released_with` asset; a changed pack is published with the current app release. Do not add a separate language-pack update workflow or probe releases one by one at runtime.
- A language pack may translate only user-visible presentation text. It must not change Technical Specs keys, Tag or Ownership values, JSON/API schemas, NFO structure, cache/index keys, patch markers, task protocols, or other machine-readable identifiers. Validate coverage, protected tokens, target scripts, and catalog/hash alignment before release.
- Preserve existing user data across localization upgrades. Never rewrite caches, derived indexes, NFO files, ownership metadata, backups, or old log bytes merely because the locale system changed. Capture the effective locale when a task starts so all new logs, progress, errors, summaries, and warnings from that task use one language even if the UI language changes while it runs.
- The language picker uses fixed rectangular flag assets, never emoji; both Chinese variants use the People's Republic of China flag. It has no table header. Each row shows the language name in the current UI language on the left and its native name on the right, except that the active language is not duplicated. Uninstalled packs use a universally recognizable download icon rather than a text-only Download label.
- Keep the Manager Web UI and Windows tray/native-dialog language synchronized. A verified Manager pack and its derived Web Card language file are separate visible outcomes; failure to publish the Web Card locale must not invalidate the installed Manager language.
- Keep update failures structurally distinct and locally actionable: offline/DNS/proxy failures, primary rate limiting, secondary throttling, unexplained HTTP 403 responses, missing or mismatched assets, and download failures must not collapse into one generic error.
- Keep the Web Card locale mapping in an extensible presentation registry. `zh-CN` displays Chinese labels, `en-US` displays English labels, and unsupported or unavailable Emby locales fall back to Simplified Chinese. Stable fields such as `Camera` and `Sound mix` remain protocol keys; only their display labels, ARIA labels, and empty-state text are localized.
- Preserve the responsive layout invariants in every supported language: the wide console uses the five-card horizontal layout; an orphaned small card must trigger the intentional balanced fallback instead of occupying a row alone; NFO search and status controls stay on one row even at extreme narrow widths and never become a three-row toolbar.
- Keep the header brand block indivisible: logo, product title, version, and subtitle retain their specified typography and must not shrink, split, or reorder. At medium widths, Refresh and Settings stay at the upper right with status badges on the following line; only the extreme-narrow layout may fully stack them. Breakpoint transitions must be monotonic and must not bounce back to an earlier layout or add a hover/lifecycle lift.

## README stewardship and release documentation

- Codex owns ongoing README maintenance and cross-language synchronization for this repository. Keep the authoritative default Simplified Chinese `README.md` at the repository root and keep every other localized README together under `docs/readme/`; do not scatter localized variants through the root.
- Every README variant must start with the same language selector. The selector and localized README inventory must match the application's supported-language registry. Adding, removing, or renaming an app locale requires the corresponding README and selector change in every variant.
- Before every formal release, audit the current source, packaging recipes, actual release asset names, supported platforms/locales, screenshots, installation and update behavior, security and compatibility notes, known limitations, and roadmap against every README. Remove or revise stale claims before publishing.
- At release time, inspect changes to the root and all localized READMEs since the preceding release. A maintainer's manual edit to any language version is intentional source material: preserve it, determine its semantic effect, and propagate that effect to every other language rather than overwriting it from a presumed master copy. Locale-specific wording may differ, but product facts, links, version scope, and roadmap state must stay aligned.
- Validate all README language-selector, image, document, license, and release links after moving or editing documentation. Publish release notes in both Chinese and English.
- Update Core, Web, native User-Agent, build scripts, tests, release documents, and README version claims only during final release closeout, after the feature and regression scope is complete. Do not let unfinished intermediate source claim the target release version.

## Verification and release boundary

- For relevant changes, run all Windows Python contract/regression tests, JavaScript syntax checks, Go tests and vet, architecture checks, and the Windows x64 GUI cross-build. Report commands, results, migrations, modified files, and all untested real-platform items.
- Preserve regression coverage for LOWORD exit-code handling, BOM/newline-safe Web Card patching, read-only NFO behavior, legacy compatibility, elevation results, Movie/TV scope, and lifecycle cleanup.
- Real Windows x64 tray/process behavior, PowerShell 5.1, UAC/elevation, login startup, browser loading, and real Emby DOM/installation behavior are explicit acceptance boundaries; cross-compilation cannot satisfy them.
- The current formal Windows deliverable is a GUI x64 `.exe` contained in a ZIP. Never place a bare `.exe` in `releases/`. Do not infer packaging rules for a future platform port; define and verify them when that port is explicitly undertaken.
- Audit the exact release range and all primary flows before packaging. Create a new version rather than overwrite an artifact.
- Do not build a release package, create a tag, push, or publish a release unless the maintainer explicitly requests that release after review. A successful build alone is not release approval.
- Do not add the phrase “修复一些提交初期的草台班子问题” to release notes unless the maintainer explicitly asks for that exact phrase for that release.

## Release naming, tags, and update-package compatibility

- The GitHub Release title must be exactly the canonical tag `vX.Y.Z`. Do not prefix or suffix it with the product name, platform, architecture, package type, or descriptive text. This title rule does not change canonical release-asset filenames.
- Use the short, canonical artifact base for every official Tech Card Manager release: `TCM-vX.Y.Z-Windows-x64-EXE` for a Windows x64 portable EXE ZIP and `TCM-vX.Y.Z-MacOS-AArch64-APP` for a future macOS arm64 app ZIP. The ZIP filename is the base plus `.zip`.
- Companion assets must use the same base: `-SHA256SUMS.txt` for checksums, `-README.txt` for release instructions, and `-CHANGELOG.txt` for release notes. A future macOS OTA package additionally uses `.zip.sig`. Never restore the older long product-name-first artifact convention.
- Keep the installed portable executable name stable as `Tech-Card-Manager.exe`; the version belongs in the archive name, not in the executable a user replaces.
- The build recipe, checksum manifest, GitHub Release assets, update selector, update UI, and tests must agree on the exact canonical filename for the tag being released. The update check must reject an absent, incorrectly named, wrong-platform, draft, or prerelease package rather than report a usable update.
- Before publishing, verify the actual GitHub Release API response: the newest stable `vX.Y.Z` release must expose the exact expected Windows package before the UI can direct a user to download it.
- Do not tag unreleased work. After the maintainer approves the complete source range for a formal release, place exactly its matching `vX.Y.Z` tag on the final release commit immediately before pushing. Never create or push a preliminary tag, move or reuse a published tag, or overwrite a published release.
- If an already-published asset needs only a filename correction, rename it in place through the GitHub Release asset API after comparing its digest before and after. Do not re-upload, rebuild, alter its bytes, or silently change release contents.
