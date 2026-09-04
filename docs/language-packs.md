# Language packs

Tech Card Manager keeps `zh-CN`, `zh-Hant`, and `en-US` in the portable executable. Other registered languages are release assets and are installed in `data/language-packs/` beside the portable application.

`language-packs/catalog.json` is the release source of truth. The byte-identical copy at `windows/language_catalog.json` is embedded in the executable. A catalog maps every locale to one exact revision, original GitHub Release tag, asset name, SHA-256 digest, and message-set hash. A later app release may retain an older `released_with` value when that locale did not change; the client never scans historical Releases.

The embedded catalog is active only when its `app_version` exactly equals the running app version. This lets the next release catalog be reviewed while source version labels still identify the last completed release, without allowing an older binary to download future packs. The final version closeout activates the matching catalog.

At startup the app restores the configured external language and every other external locale that has an older installed revision. Locales the user never downloaded are not fetched automatically. Consequently an app upgrade updates all previously installed packs to the exact revisions in its catalog without exposing a separate language-pack update action.

Human-reviewable translations live at `language-packs/<locale>/r<revision>/translations.json`. `tools/build-language-packs.py` converts stable English presentation strings to stable IDs and creates deterministic ZIP bytes. Changed locales receive a new revision and an asset on the current app Release; unchanged descriptors continue pointing to their original Release.

The `web-card` section is published as the app-owned `technical-specs-languages.json` next to the installed Web Card. Emby resolves only registered locales with installed translations; every other Emby language continues to fall back to Simplified Chinese. Restoring native Emby removes this derived language file with the other owned Web Card files.

Installing the Manager language pack and publishing the derived Web Card language file are separate visible outcomes. A Web Card permission failure does not roll back or disable an already verified Manager language; the UI reports the publication warning and later runtime-state writes retry it.

Run `python3 tools/build-language-packs.py --update-catalog` after an intentional translation or revision change, review both catalog copies, then run it again without that flag. Official packaging supplies the exact app version and emits only newly published language assets.

Normal source checks permit translators to fill a revision incrementally. Formal packaging additionally passes `--require-complete`, which extracts the registered Manager Web UI, Core/Engine, native, and Web Card message inventories and refuses a release while any downloadable locale is missing a key. Development can still fall back to English for an unfinished string, but a formal v4.1.0 package cannot be produced in that state.

Language packs are presentation-only. They cannot change media NFO bytes, Technical Specs field keys, ownership evidence, cache/index schemas, JSON protocols, or the product’s read-only boundary. Old installed revisions and historical logs are preserved for upgrade and rollback continuity.

The ZIP digest authenticates the downloaded archive. Its authenticated manifest also records a SHA-256 digest for every extracted section; the app rechecks those files before treating a pack as installed. A missing, modified, wrong-product, wrong-release, wrong-revision, or wrong-schema pack is rejected and can be atomically restored from the exact release asset. The `native` section supplies Windows tray menus and native dialogs; `web-card` remains a separate presentation registry and never changes Technical Specs keys.
