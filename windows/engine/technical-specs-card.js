(() => {
    "use strict";

    const WEB_CARD_VERSION = "4.0.1";

    // Single-run guard: if another copy of this card script (older or equal)
    // is already active on the page, only the newest version may run. Old
    // copies must never render alongside the current renderer.
    const activeVersion = window.__itmTechCardActiveVersion || "";
    const versionRank = value => {
        const parts = String(value || "").split(".").map(x => parseInt(x, 10) || 0);
        return (parts[0] || 0) * 1000000 + (parts[1] || 0) * 1000 + (parts[2] || 0);
    };
    if (activeVersion && versionRank(activeVersion) >= versionRank(WEB_CARD_VERSION)) {
        return;
    }
    if (typeof window.__itmTechCardTeardown === "function") {
        try { window.__itmTechCardTeardown(); } catch (_) {}
    }
    window.__itmTechCardActiveVersion = WEB_CARD_VERSION;

    window.__technicalSpecsCardVersion = WEB_CARD_VERSION;
    function setTechnicalSpecsDebug(details = {}) {
        window.__technicalSpecsDebug = {
            version: WEB_CARD_VERSION,
            ...details
        };
    }

    function debugFailure(reason, details = {}) {
        setTechnicalSpecsDebug({
            ...details,
            rendered: false,
            retryReason: reason
        });
        return reason;
    }

    setTechnicalSpecsDebug({ loaded: true, rendered: false, stage: "loaded" });

    /*
     * Emby 4.9.5.0 Technical Specs integration.
     *
     * Normal files:
     *   Locate the real native Video MediaStream card inside the current
     *   visible detail root and insert the Technical Specs card in the same
     *   horizontal scroller at the measured native width.
     *
     * ISO / BDMV / Series:
     *   Create a single responsive card in the current detail page's existing
     *   media scroller, or in an isolated custom host near that media section.
     *
     *
     * SPA identity invariant:
     *   Emby may keep the previous movie's hidden detail DOM alive during
     *   navigation. Always resolve the current route item through ApiClient
     *   first, scope DOM fallback to the visible detail page, and reject stale
     *   async renders after a route change.
     *
     * SPA refresh reliability invariant:
     *   Route epochs are no longer invalidated by every DOM mutation. History
     *   pushState/replaceState is observed directly, mutation storms are
     *   leading-edge coalesced, and a bounded retry window handles provider ID,
     *   index JSON and MediaStream DOM that become ready after navigation.
     *
     * Conservative stability baseline:
     *   Keep the established render/scheduling core proven on the target Emby.
     *   Add only the product rules that are required: Series home-page support,
     *   Episode/Season suppression, safer Video-card detection, faster index
     *   refresh on misses, and a lightweight stale-item watchdog.
     *
     * Native structures verified from the user's installed Emby 4.9.5.0:
     *
     * item.js:
     *   detailMediaStreamsItemsContainer
     *   cardbuilder.buildCards(... fields:["MediaStreamInfo"] ...)
     *   cardPadderClass: "mediaStreamPadder"
     *   innerCardFooterClass: "mediaStreamInnerCardFooter"
     *   cardTextCssClass: "mediaStreamInnerCardFooter-cardText"
     *
     * mediainfo.js:
     *   <h3 ...><i class="md-icon autortl mediaStreamTypeIcon">...</i>...</h3>
     *   <div class="flex mediaStreamAttribute">
     *     <span class="mediaInfoAttributeLabel">...</span>
     *     <span class="mediaInfoAttributeValue secondaryText">...</span>
     *   </div>
     */

    const SCRIPT_URL = document.currentScript && document.currentScript.src
        ? document.currentScript.src
        : location.href;

    const BASE_URL = new URL(".", SCRIPT_URL).href;
    const DATA_URL = new URL("technical-specs-data.json", BASE_URL).href;
    const RUNTIME_URL = new URL("technical-specs-runtime.json", BASE_URL).href;
    const DEFAULT_STANDARD_CARD_WIDTH_PX = 278;
    const CARD_LEASE_POLL_MS = 1200;

    const FIELD_ORDER = [
        "Runtime",
        "Sound mix",
        "Color",
        "Aspect ratio",
        "Camera",
        "Laboratory",
        "Film Length",
        "Negative Format",
        "Cinematographic Process",
        "Printed Film Format"
    ];

    const ZH_LABELS = {
        "Runtime": "正片时长",
        "Sound mix": "声音制式",
        "Color": "色彩类型",
        "Aspect ratio": "画幅比例",
        "Camera": "摄影器材",
        "Laboratory": "冲印流程",
        "Film Length": "胶片长度",
        "Negative Format": "底片格式",
        "Cinematographic Process": "摄影工艺",
        "Printed Film Format": "放映格式"
    };

    const TECH_CARD_SELECTOR = "[data-tech-spec-card='1']";
    const NATIVE_HOST_SELECTOR = "[data-tech-spec-native-host='1']";

    const OTHER_INFO_HEADINGS = new Set([
        "其他信息",
        "其它信息",
        "其他資訊",
        "其它資訊",
        "other information",
        "additional information",
        "additional info",
        "more"
    ]);

    const VIDEO_HEADINGS = new Set([
        "video", "视频", "影片", "視訊", "视讯"
    ]);

    const CARD_ELIGIBLE_TYPES = new Set(["Movie", "Series"]);
    const CARD_SUPPRESSED_TYPES = new Set(["Episode", "Season"]);

    let renderTimer = 0;
    let renderRequestId = 0;
    let renderInFlight = false;
    let pendingRender = false;
    let retryTimer = 0;
    let retryAttempt = 0;
    let cachedData = null;
    let cachedAt = 0;
    let lastRenderKey = "";
    let lastLocationKey = "";
    let lastRouteItemId = "";
    let lastRenderOutcome = "idle";
    let observer = null;
    let watchdogInterval = 0;
    let leaseInterval = 0;
    let leaseCheckInFlight = null;
    let cachedRuntime = null;
    let cachedRuntimeValidUntil = 0;
    let textMeasureContext = null;
    const cardHeightObservers = new Map();
    const cardHeightStyleSnapshots = new Map();
    const cardLayoutBaselines = new WeakMap();
    const cardLayoutSignatures = new WeakMap();
    const cardLayoutSyncPending = new WeakSet();
    const cleanupCallbacks = [];
    const historyRestores = [];

    // A detail view can exist before ApiClient/provider IDs/media-stream DOM are
    // ready. Retry through the whole SPA settling window instead of requiring
    // the user to refresh the browser. Total window is ~32 seconds.
    const RETRY_DELAYS_MS = [
        150, 300, 600, 1000, 1800, 3000, 5000, 8000, 12000
    ];

    function getLocationKey() {
        return [
            String(location.pathname || ""),
            String(location.search || ""),
            String(location.hash || "")
        ].join("|");
    }

    function renderRequestStillCurrent(requestId, locationKey) {
        return (
            requestId === renderRequestId &&
            locationKey === getLocationKey()
        );
    }

    function normalizeText(value) {
        return String(value || "").replace(/\s+/g, " ").trim();
    }

    function uiIsChinese() {
        const lang = String(document.documentElement.lang || "").toLowerCase();
        if (lang.startsWith("zh")) return true;

        const culture =
            document.body && (
                document.body.getAttribute("data-culture") ||
                document.body.getAttribute("lang")
            );

        return String(culture || "").toLowerCase().startsWith("zh");
    }

    function decodeRouteValue(value) {
        try {
            return decodeURIComponent(value).trim();
        } catch (_) {
            return String(value || "").trim();
        }
    }

    function getRouteItemId() {
        const href = String(location.href || "");

        // Emby item IDs are not guaranteed to be GUID-like. The route is the
        // strongest identity signal because hidden SPA pages can remain in DOM.
        const queryMatch = href.match(/[?&#](?:id|itemid)=([^&#/?]+)/i);
        if (queryMatch && queryMatch[1]) {
            return decodeRouteValue(queryMatch[1]);
        }

        const pathMatch = href.match(/\/items\/([^/?#]+)/i);
        if (pathMatch && pathMatch[1]) {
            return decodeRouteValue(pathMatch[1]);
        }

        return "";
    }

    function getItemId() {
        const routeId = getRouteItemId();
        if (routeId) return routeId;

        // DOM is only a fallback. Emby keeps old item pages in the SPA tree, so
        // never accept identity nodes from hidden views.
        const candidates = document.querySelectorAll(
            "[data-itemid], [data-item-id], [data-id]"
        );

        for (const node of candidates) {
            if (!isInVisibleTree(node)) continue;

            const value = String(
                node.getAttribute("data-itemid") ||
                node.getAttribute("data-item-id") ||
                node.getAttribute("data-id") ||
                ""
            ).trim();

            if (value) return value;
        }

        return "";
    }

    function getImdbFromDom(scope) {
        // DOM provider links are deliberately a LAST fallback. Emby is a SPA
        // and may retain the previous movie's hidden detail page. A legacy
        // implementation searched the whole document and could attach that IMDb
        // record to the new movie's freshly rendered MediaStream cards.
        const root = scope || findVisibleDetailRoot() || document;

        for (const anchor of root.querySelectorAll("a[href]")) {
            if (!isInVisibleTree(anchor)) continue;

            const href = String(anchor.getAttribute("href") || anchor.href || "");
            const match = href.match(/imdb\.com\/title\/(tt\d{5,12})/i);
            if (match) return match[1].toLowerCase();
        }

        // Some themes render the provider id as plain text. Scope this search to
        // the currently visible detail root for the same anti-stale guarantee.
        const text = String(root && root.innerText || "");
        const match = text.match(/\btt\d{5,12}\b/i);
        return match ? match[0].toLowerCase() : "";
    }

    async function getCurrentItem(id) {
        if (!window.ApiClient || !id) return null;

        try {
            if (
                typeof ApiClient.getCurrentUserId === "function" &&
                typeof ApiClient.getItem === "function"
            ) {
                const userId = ApiClient.getCurrentUserId();
                if (userId) {
                    return await ApiClient.getItem(userId, id);
                }
            }
        } catch (_) {}

        try {
            if (
                typeof ApiClient.getCurrentUserId === "function" &&
                typeof ApiClient.getUrl === "function"
            ) {
                const userId = ApiClient.getCurrentUserId();
                if (!userId) return null;

                const url = ApiClient.getUrl(`Users/${userId}/Items/${id}`);
                const response = await fetch(url, {
                    credentials: "same-origin"
                });

                if (response.ok) {
                    return await response.json();
                }
            }
        } catch (_) {}

        return null;
    }

    async function getTechDatabase(force = false) {
        const now = Date.now();

        if (!force && cachedData && now - cachedAt < 5000) {
            return cachedData;
        }

        try {
            const response = await fetch(`${DATA_URL}?v=${now}`, {
                cache: "no-store",
                credentials: "same-origin"
            });

            if (!response.ok) return null;

            cachedData = await response.json();
            cachedAt = now;
            return cachedData;
        } catch (_) {
            return null;
        }
    }

    function monotonicNow() {
        return window.performance && typeof window.performance.now === "function"
            ? window.performance.now()
            : Date.now();
    }

    function runtimeLeaseShapeIsValid(runtimeState) {
        if (!runtimeState || runtimeState.enabled !== true) return false;
        if (String(runtimeState.web_card_version || "") !== WEB_CARD_VERSION) return false;
        const updatedAt = Date.parse(String(runtimeState.updated_at || ""));
        const expiresAt = Date.parse(String(runtimeState.expires_at || ""));
        const lifetime = expiresAt - updatedAt;
        return Number.isFinite(updatedAt) && Number.isFinite(expiresAt) &&
            lifetime > 0 && lifetime <= 30000;
    }

    function runtimeLeaseIsValid(runtimeState) {
        return runtimeLeaseShapeIsValid(runtimeState) &&
            monotonicNow() < cachedRuntimeValidUntil;
    }

    function acceptRuntimeLease(runtimeState, response) {
        cachedRuntime = runtimeState;
        cachedRuntimeValidUntil = 0;
        if (!runtimeLeaseShapeIsValid(runtimeState)) return false;

        const expiresAt = Date.parse(String(runtimeState.expires_at || ""));
        const updatedAt = Date.parse(String(runtimeState.updated_at || ""));
        const serverDate = Date.parse(String(response.headers.get("Date") || ""));
        const referenceNow = Number.isFinite(serverDate) ? serverDate : Date.now();
        const remaining = expiresAt - referenceNow;
        const declaredLifetime = expiresAt - updatedAt;
        if (remaining <= 0) return false;

        // Convert the server-authored wall-clock lease into a local monotonic
        // deadline. This tolerates a client device whose clock differs from the
        // Windows Emby server, while a stale runtime file still expires against
        // the HTTP server's Date header.
        cachedRuntimeValidUntil = monotonicNow() + Math.min(remaining, declaredLifetime);
        return true;
    }

    async function refreshRuntimeLease(force = false) {
        if (!force && runtimeLeaseIsValid(cachedRuntime)) return true;
        if (leaseCheckInFlight) return leaseCheckInFlight;

        leaseCheckInFlight = (async () => {
            try {
                const response = await fetch(`${RUNTIME_URL}?v=${Date.now()}`, {
                    cache: "no-store",
                    credentials: "same-origin"
                });
                if (!response.ok) {
                    return runtimeLeaseIsValid(cachedRuntime);
                }
                const state = await response.json();
                return acceptRuntimeLease(state, response);
            } catch (_) {
                // A transient fetch failure may use the last unexpired lease,
                // but can never extend it. Once the Manager stops renewing,
                // every open page removes its owned card automatically.
                return runtimeLeaseIsValid(cachedRuntime);
            } finally {
                leaseCheckInFlight = null;
            }
        })();

        return leaseCheckInFlight;
    }

    async function enforceRuntimeLease(reason) {
        const allowed = await refreshRuntimeLease(true);
        if (!allowed) {
            removeOldTechnicalCards();
            lastRenderKey = "";
            lastRenderOutcome = "service-stopped";
            window.__technicalSpecsDebug = {
                version: WEB_CARD_VERSION,
                rendered: false,
                serviceLeaseValid: false,
                reason: reason || "service-stopped"
            };
            return false;
        }
        return true;
    }

    function isInVisibleTree(element) {
        if (!element || !element.isConnected) return false;

        let node = element;
        while (node && node.nodeType === Node.ELEMENT_NODE) {
            if (node.classList && node.classList.contains("hide")) {
                return false;
            }

            try {
                const style = window.getComputedStyle(node);
                if (
                    style.display === "none" ||
                    style.visibility === "hidden" ||
                    style.visibility === "collapse"
                ) {
                    return false;
                }
            } catch (_) {}

            node = node.parentElement;
        }

        return true;
    }

    function isNativeVideoIcon(icon) {
        if (!icon) return false;

        const value = icon.textContent || "";
        if (!value) return false;

        /*
         * Emby 4.9.5.0 BaseItemController:
         * defaultIconsByItemType.Video = &#xe02c;
         *
         * We use this only to LOCATE the native Video card. The normal-path
         * Technical Specs icon is inherited by cloning the actual native
         * Video heading. The same known glyph is used only by the ISO fallback.
         */
        return value.codePointAt(0) === 0xe02c;
    }

    function getNativeMediaStreamContainers(root) {
        if (!root) return [];
        return [...root.querySelectorAll(".detailMediaStreamsItemsContainer")]
            .filter(container => (
                isInVisibleTree(container) &&
                !container.closest(NATIVE_HOST_SELECTOR)
            ));
    }

    function findNativeVideoCard(root) {
        const containers = getNativeMediaStreamContainers(root);

        // First try the exact native Video icon from Emby 4.9.5.0.
        for (const container of containers) {
            const icons = container.querySelectorAll(
                ".mediaStreamTypeIcon"
            );

            for (const icon of icons) {
                if (!isNativeVideoIcon(icon)) continue;

                const heading = icon.closest("h3");
                const card = icon.closest(".card");
                const footer =
                    card &&
                    card.querySelector(
                        ".mediaStreamInnerCardFooter"
                    );

                if (heading && card && footer &&
                    !card.hasAttribute("data-tech-spec-card")) {
                    return { container, card, heading, footer };
                }
            }
        }

        // Theme-safe fallback: only clone a card whose heading itself says
        // Video. Never assume the first MediaStream card is video; ISO/BDMV
        // items may expose only audio/subtitle cards.
        for (const container of containers) {
            const cards = container.querySelectorAll(".card");

            for (const card of cards) {
                if (card.hasAttribute("data-tech-spec-card")) continue;
                const footer = card.querySelector(".mediaStreamInnerCardFooter");
                const heading = footer && footer.querySelector("h3");
                if (!footer || !heading) continue;

                let raw = String(heading.textContent || "");
                const icon = heading.querySelector(".mediaStreamTypeIcon");
                if (icon && icon.textContent) raw = raw.replace(icon.textContent, "");
                const text = normalizeText(raw).toLowerCase();
                if (VIDEO_HEADINGS.has(text)) {
                    return { container, card, heading, footer };
                }
            }
        }

        return null;
    }

    function findVisibleNativeMediaContainer(root) {
        const containers = getNativeMediaStreamContainers(root);
        return containers.length ? containers[0] : null;
    }

    function findAnyNativeMediaCard(root) {
        for (const container of getNativeMediaStreamContainers(root)) {
            for (const card of container.querySelectorAll(".card")) {
                if (card.hasAttribute("data-tech-spec-card")) continue;
                const footer = card.querySelector(".mediaStreamInnerCardFooter");
                if (!footer) continue;
                return {
                    container,
                    card,
                    footer,
                    heading: footer.querySelector("h3")
                };
            }
        }
        return null;
    }

    function ensureCardStyles() {
        if (document.getElementById("tech-spec-card-style")) return;
        const style = document.createElement("style");
        style.id = "tech-spec-card-style";
        style.textContent = [
            // The outer shell is Emby's real media-stream card structure. The
            // only Manager-owned rules are sizing and multi-value line breaks;
            // background, border, type, padding and layout remain Emby-native.
            "[data-tech-spec-card='1']{--itm-standard-card-width:278px;--itm-wide-card-max-width:556px;flex:0 0 auto!important;align-self:flex-start;box-sizing:border-box}",
            "[data-tech-spec-card='1'].itm-tech-card--native{width:var(--itm-standard-card-width)!important;min-width:var(--itm-standard-card-width)!important;max-width:var(--itm-standard-card-width)!important}",
            "[data-tech-spec-card='1'].itm-tech-card--wide{width:fit-content!important;min-width:min(var(--itm-standard-card-width),calc(100vw - 3rem))!important;max-width:min(var(--itm-wide-card-max-width),calc(100vw - 3rem))!important}",
            // Emby's mediaStreamPadder applies a portrait-like 16/29.5 ratio.
            // That ratio makes a tall MKV card grow sideways and makes a wide
            // ISO card grow far below its text. Neutralise it only inside the
            // Manager-owned clone; native Video/Audio cards stay untouched.
            "[data-tech-spec-card='1']{height:auto!important;max-height:none!important}",
            "[data-tech-spec-card='1'] .cardBox{max-width:100%!important;height:auto!important;max-height:none!important;box-sizing:border-box!important}",
            "[data-tech-spec-card='1'] .cardContent{width:100%!important;max-width:100%!important;min-width:0!important;height:auto!important;max-height:none!important;aspect-ratio:auto!important;display:block!important;box-sizing:border-box!important;overflow:hidden!important}",
            "[data-tech-spec-card='1'] .mediaStreamInnerCardFooter{width:100%!important;max-width:100%!important;min-width:0!important;height:auto!important;max-height:none!important;display:block!important;box-sizing:border-box!important;overflow:visible!important}",
            // Emby's value column may inherit nowrap/min-width:auto from the
            // source media card. It must be the shrinkable flex child or text
            // is clipped at the card edge instead of wrapping there.
            "[data-tech-spec-card='1'] .mediaStreamInnerCardFooter-cardText{width:100%!important;height:auto!important;min-width:0!important;max-width:100%!important;white-space:normal!important;overflow:visible!important;text-overflow:clip!important;box-sizing:border-box!important}",
            "[data-tech-spec-card='1'] .mediaStreamAttribute{display:flex!important;width:100%!important;min-width:0!important;align-items:flex-start!important;box-sizing:border-box!important}",
            "[data-tech-spec-card='1'] .mediaInfoAttributeLabel{flex:0 0 auto!important;min-width:0!important;white-space:nowrap!important}",
            "[data-tech-spec-card='1'] .mediaInfoAttributeValue{display:block!important;flex:1 1 0!important;min-width:0!important;max-width:100%!important;width:auto!important;white-space:normal!important;overflow-wrap:anywhere!important;word-break:normal!important;overflow:visible!important;text-overflow:clip!important}",
            "[data-tech-spec-card='1'] .itm-tech-spec-value-line{display:block!important;width:100%!important;min-width:0!important;max-width:100%!important;white-space:normal!important;overflow-wrap:anywhere!important;word-break:normal!important;overflow:visible!important;text-overflow:clip!important}",
            "[data-tech-spec-native-host='1']{margin:0}",
            // Legacy cleanup rules from <=2.16 fallback shells, kept so stale
            // inline styles can still be cleared on page updates.
            "[data-tech-spec-card].techSpecsFallbackShell .cardBox,[data-tech-spec-card].techSpecsFallbackShell .cardScalable,[data-tech-spec-card].techSpecsFallbackShell .cardContent,[data-tech-spec-card].techSpecsFallbackShell .cardPadder{display:block;width:auto;height:auto;padding:0;margin:0;background:transparent}"
        ].join("");
        document.head.appendChild(style);
    }

    function measureNativeCardWidth(sourceCard) {
        try {
            if (sourceCard && sourceCard.isConnected) {
                const width = parseFloat(getComputedStyle(sourceCard).width);
                if (width > 120 && width < 900) {
                    return Math.round(width);
                }
            }
        } catch (err) { /* measurement is best-effort */ }
        return null;
    }

    function findLiveNativeReferenceCard(card) {
        const ownContainer = card && card.closest(
            ".detailMediaStreamsItemsContainer"
        );
        if (ownContainer && !ownContainer.closest(NATIVE_HOST_SELECTOR)) {
            const sibling = [...ownContainer.querySelectorAll(".card")].find(
                node => node !== card &&
                    !node.hasAttribute("data-tech-spec-card") &&
                    isInVisibleTree(node)
            );
            if (sibling) return sibling;
        }

        const root = findVisibleDetailRoot();
        const native = root && findAnyNativeMediaCard(root);
        return native && native.card !== card ? native.card : null;
    }

    function resolvedStandardCardWidth(card) {
        const liveWidth = measureNativeCardWidth(
            findLiveNativeReferenceCard(card)
        );
        if (liveWidth) return liveWidth;
        const baseWidth = parseFloat(
            card && card.getAttribute("data-tech-spec-base-width") || ""
        );
        return baseWidth > 120 && baseWidth < 900
            ? Math.round(baseWidth)
            : DEFAULT_STANDARD_CARD_WIDTH_PX;
    }

    function elementHeight(node) {
        if (!node) return 0;
        try {
            const height = node.getBoundingClientRect().height;
            return height > 0 ? Math.ceil(height) : 0;
        } catch (_) {
            return 0;
        }
    }

    function captureNativeCardBaseline(sourceCard) {
        if (!sourceCard || !sourceCard.isConnected) return null;
        return {
            width: Math.max(0, sourceCard.getBoundingClientRect().width || sourceCard.clientWidth || 0),
            card: elementHeight(sourceCard),
            box: elementHeight(sourceCard.querySelector(".cardBox")),
            content: elementHeight(sourceCard.querySelector(".cardContent")),
            footer: elementHeight(sourceCard.querySelector(
                ".mediaStreamInnerCardFooter"
            ))
        };
    }

    function removeInteractiveCardState(card) {
        const nodes = [card, ...card.querySelectorAll("*")];
        for (const node of nodes) {
            for (const name of [
                "data-id", "data-index", "data-action", "data-contextmenu",
                "data-playaction", "href", "onclick", "tabindex"
            ]) {
                node.removeAttribute(name);
            }
            if (node.classList) {
                node.classList.remove("card-hoverable", "itemAction", "focusable");
            }
        }
    }

    function createCapturedEmbyCardShell() {
        // Exact non-interactive shell collected from the user's Emby 4.9.5.0
        // Video card. This path is used only when ISO/BDMV/Series has no live
        // media card to clone on the current page.
        const card = document.createElement("div");
        card.className = "card backdropCard card-horiz backdropCard-horiz";
        const box = document.createElement("div");
        box.className = "cardBox cardBox-touchzoom";
        const content = document.createElement("div");
        content.className = "cardContent cardImageContainer cardContent-background cardContent-bxsborder-fv defaultCardBackground cardPadder-backdrop mediaStreamPadder";
        const footer = document.createElement("div");
        footer.className = "innerCardFooter mediaStreamInnerCardFooter";
        content.appendChild(footer);
        box.appendChild(content);
        card.appendChild(box);
        return card;
    }

    function replaceHeadingText(heading, title) {
        const icon = heading.querySelector(".mediaStreamTypeIcon");
        const clonedIcon = icon ? icon.cloneNode(true) : null;
        heading.replaceChildren();
        if (clonedIcon) heading.appendChild(clonedIcon);
        heading.appendChild(document.createTextNode(title));
    }

    function appendNativeSpecFooter(footer, templateHeading, specs, zh) {
        const titleRow = document.createElement("div");
        titleRow.className = "mediaStreamInnerCardFooter-cardText cardText text-align-start innerFooter-cardText cardText-first-padded";
        const heading = templateHeading
            ? templateHeading.cloneNode(true)
            : document.createElement("h3");
        if (!templateHeading) {
            heading.className = "flex align-items-center";
            heading.style.margin = ".6em 0 .8em";
        }
        replaceHeadingText(heading, zh ? "技术规格" : "Technical Specs");
        titleRow.appendChild(heading);
        footer.appendChild(titleRow);

        let groups = 0;
        for (const field of FIELD_ORDER) {
            const values = Array.isArray(specs[field])
                ? specs[field].map(normalizeText).filter(Boolean)
                : [];
            if (!values.length) continue;

            const rowHost = document.createElement("div");
            rowHost.className = "mediaStreamInnerCardFooter-cardText cardText text-align-start innerFooter-cardText";
            const row = document.createElement("div");
            row.className = "flex mediaStreamAttribute";
            const label = document.createElement("span");
            label.className = "mediaInfoAttributeLabel";
            label.textContent = zh ? (ZH_LABELS[field] || field) : field;
            const valueHost = document.createElement("span");
            valueHost.className = "mediaInfoAttributeValue secondaryText";
            for (const value of values) {
                const line = document.createElement("span");
                line.className = "itm-tech-spec-value-line";
                line.textContent = value;
                valueHost.appendChild(line);
            }
            row.append(label, valueHost);
            rowHost.appendChild(row);
            footer.appendChild(rowHost);
            groups++;
        }
        return groups;
    }

    function buildTechnicalCard(specs, zh, layoutMode, widthPx, template) {
        // Prefer a byte-for-byte DOM clone of the live Video/media card. Pages
        // without media cards use the captured Emby 4.9.5.0 native shell.
        ensureCardStyles();
        const nativeBaseline = layoutMode === "native-card" &&
            template && template.card
            ? captureNativeCardBaseline(template.card)
            : null;
        const card = template && template.card
            ? template.card.cloneNode(true)
            : createCapturedEmbyCardShell();
        removeInteractiveCardState(card);
        card.classList.add(layoutMode === "native-card"
            ? "itm-tech-card--native"
            : "itm-tech-card--wide");
        card.setAttribute("data-tech-spec-card", "1");
        card.setAttribute("data-tech-spec-render-mode", layoutMode);
        card.setAttribute("aria-label", zh ? "技术规格" : "Technical Specs");

        const standardWidth = widthPx ||
            measureNativeCardWidth(template && template.card) ||
            DEFAULT_STANDARD_CARD_WIDTH_PX;
        card.setAttribute("data-tech-spec-base-width", String(standardWidth));
        card.style.setProperty("--itm-standard-card-width", standardWidth + "px");
        card.style.setProperty("--itm-wide-card-max-width", (standardWidth * 2) + "px");
        if (nativeBaseline) cardLayoutBaselines.set(card, nativeBaseline);

        const footer = card.querySelector(".mediaStreamInnerCardFooter");
        if (!footer) return null;
        footer.replaceChildren();
        const groups = appendNativeSpecFooter(
            footer,
            template && template.heading,
            specs,
            zh
        );
        if (!groups) return null;
        if (layoutMode === "native-card") {
            card.style.width = standardWidth + "px";
        }
        return card;
    }

    function findOtherInfoHeading(root) {
        if (!root) return null;
        const candidates = root.querySelectorAll(
            "h1, h2, h3, .sectionTitle"
        );

        for (const heading of candidates) {
            if (!isInVisibleTree(heading)) continue;

            const text = normalizeText(heading.textContent).toLowerCase();
            if (OTHER_INFO_HEADINGS.has(text)) {
                return heading;
            }
        }

        return null;
    }

    function findVisibleDetailRoot() {
        const selectors = [
            // Emby 4.9.5.0 uses .itemView/view-item-item as the real detail
            // root. Older selectors below are retained for compatible themes.
            ".itemView.view-item-item:not(.hide)",
            ".itemView:not(.hide)",
            ".itemDetailPage:not(.hide)",
            ".detailPage:not(.hide)",
            ".detailPageContent",
            ".detailPageContentContainer",
            ".detailPagePrimaryContainer",
            ".detailPageWrapperContainer",
            ".itemDetailPage",
            ".detailPage",
            "main"
        ];

        for (const selector of selectors) {
            const candidates = [...document.querySelectorAll(selector)];
            for (const candidate of candidates) {
                if (isInVisibleTree(candidate)) {
                    return candidate;
                }
            }
        }

        return null;
    }

    function normalizeItemType(value) {
        const key = normalizeText(value).toLowerCase();
        return ({movie: "Movie", series: "Series", episode: "Episode", season: "Season"})[key] || "";
    }

    function getVisibleItemType(root) {
        if (!root) return "";
        const candidates = [root, ...root.querySelectorAll("[data-type], [data-item-type]")];
        for (const node of candidates) {
            if (!isInVisibleTree(node)) continue;
            const type = normalizeItemType(
                node.getAttribute("data-type") || node.getAttribute("data-item-type") || ""
            );
            if (type) return type;
        }
        return "";
    }

    function getIndexedItemType(database, imdb) {
        if (!database || !database.itemTypes || !imdb) return "";
        return normalizeItemType(
            database.itemTypes[imdb] ||
            database.itemTypes[imdb.toLowerCase()] ||
            database.itemTypes[imdb.toUpperCase()]
        );
    }

    function createNativeMediaHost() {
        // Reproduce Emby's captured media-card hierarchy, but mark the outer
        // section as Manager-owned and keep every node non-interactive.
        const section = document.createElement("div");
        section.className = "verticalSection verticalSection-cards audioVideoMediaInfo";
        section.setAttribute("data-tech-spec-native-host", "1");
        const mediaSources = document.createElement("div");
        mediaSources.className = "mediaSources";
        // Do not assign Emby's behavioural `.mediaSource` class to a generated
        // node: item.js may treat it as a real playable source. We reuse only
        // the native scroll/layout classes needed by a card collection.
        const mediaScroller = document.createElement("div");
        mediaScroller.className = "emby-scrollbuttons-scroller";
        const scroller = document.createElement("div");
        scroller.className = "emby-scroller padded-top-focusscale padded-bottom-focusscale padded-left padded-left-page padded-right scrollX hiddenScrollX scrollFrameX flex-direction-row";
        const container = document.createElement("div");
        container.className = "detailMediaStreamsItemsContainer itemsContainer-defaultCardSize scrollSlider itemsContainer focuscontainer-x scrollSliderX emby-scrollbuttons-scrollSlider";
        scroller.appendChild(container);
        mediaScroller.appendChild(scroller);
        mediaSources.appendChild(mediaScroller);
        section.appendChild(mediaSources);
        return { host: section, container };
    }

    function getOrCreateNativeTarget(root) {
        if (!root) return null;
        const nativeContainer = findVisibleNativeMediaContainer(root);
        if (nativeContainer) {
            return {
                container: nativeContainer,
                createdHost: false,
                placement: "visible-native-media-container"
            };
        }

        const existingHost = root.querySelector(NATIVE_HOST_SELECTOR);
        if (existingHost && isInVisibleTree(existingHost)) {
            return {
                container: existingHost.querySelector(
                    ".detailMediaStreamsItemsContainer"
                ),
                createdHost: true,
                placement: existingHost.getAttribute("data-tech-spec-placement") ||
                    "existing-native-host"
            };
        }

        const generated = createNativeMediaHost();
        const host = generated.host;

        // ISO/BDMV can expose an empty but visible native media container. The
        // earlier fast path returns it. If only mediaSources is present, add
        // the captured native scroller inside that visible section.
        const mediaSources = [...root.querySelectorAll(".mediaSources")]
            .find(isInVisibleTree);
        if (mediaSources) {
            const mediaScroller = host.querySelector(".emby-scrollbuttons-scroller");
            mediaScroller.setAttribute("data-tech-spec-native-host", "1");
            mediaScroller.setAttribute("data-tech-spec-placement", "visible-media-sources");
            mediaSources.appendChild(mediaScroller);
            return {
                container: generated.container,
                createdHost: true,
                host: mediaScroller,
                placement: "visible-media-sources"
            };
        }

        // Series keeps its native audio/video section under `.hide`. Never put
        // generated content inside that hidden subtree. Insert a new visible
        // native section as its sibling in the captured verticalSections host.
        const hiddenSection = [...root.querySelectorAll(
            ".verticalSection.audioVideoMediaInfo"
        )].find(section => !isInVisibleTree(section));
        if (hiddenSection && hiddenSection.parentNode &&
            isInVisibleTree(hiddenSection.parentNode)) {
            host.setAttribute("data-tech-spec-placement", "before-hidden-native-media-section");
            hiddenSection.parentNode.insertBefore(host, hiddenSection);
            return {
                container: generated.container,
                createdHost: true,
                host,
                placement: "before-hidden-native-media-section"
            };
        }

        // ISO-safe placement: "Other information" is present on normal movie
        // detail pages even when no MediaStream cards exist. Put our host
        // immediately before that heading/section so it stays in the detail
        // content flow and does not depend on stream probing.
        const otherInfoHeading = findOtherInfoHeading(root);
        if (otherInfoHeading && otherInfoHeading.parentNode) {
            const section = otherInfoHeading.closest(
                "section, .detailSection, .verticalSection"
            );
            const anchor =
                section && section.parentNode ? section : otherInfoHeading;

            host.setAttribute("data-tech-spec-placement", "before-other-info");
            anchor.parentNode.insertBefore(host, anchor);
            return {
                container: generated.container,
                createdHost: true,
                host,
                placement: "before-other-info"
            };
        }

        // Fail closed unless a known visible Emby detail-content anchor exists.
        const detailSections = [...root.querySelectorAll(
            ".verticalSections, .details-additionalContent, .aboutSection"
        )].find(isInVisibleTree);
        if (detailSections) {
            host.setAttribute("data-tech-spec-placement", "visible-detail-sections");
            detailSections.appendChild(host);
            return {
                container: generated.container,
                createdHost: true,
                host,
                placement: "visible-detail-sections"
            };
        }

        return null;
    }

    function setOwnedHeightStyle(card, node, property, value) {
        if (!card || !node || !node.style) return;
        if (
            node.style.getPropertyValue(property) === value &&
            node.style.getPropertyPriority(property) === "important"
        ) {
            return;
        }
        let nodeSnapshots = cardHeightStyleSnapshots.get(card);
        if (!nodeSnapshots) {
            nodeSnapshots = new Map();
            cardHeightStyleSnapshots.set(card, nodeSnapshots);
        }
        let propertySnapshots = nodeSnapshots.get(node);
        if (!propertySnapshots) {
            propertySnapshots = new Map();
            nodeSnapshots.set(node, propertySnapshots);
        }
        if (!propertySnapshots.has(property)) {
            propertySnapshots.set(property, {
                value: node.style.getPropertyValue(property),
                priority: node.style.getPropertyPriority(property)
            });
        }
        node.style.setProperty(property, value, "important");
    }

    function resetTechnicalCardHeight(card) {
        const nodeSnapshots = card && cardHeightStyleSnapshots.get(card);
        if (!nodeSnapshots) return;
        for (const [node, propertySnapshots] of nodeSnapshots) {
            if (!node || !node.style) continue;
            for (const [property, previous] of propertySnapshots) {
                if (previous.value) {
                    node.style.setProperty(
                        property,
                        previous.value,
                        previous.priority
                    );
                } else {
                    node.style.removeProperty(property);
                }
            }
        }
        cardHeightStyleSnapshots.delete(card);
    }

    function measureUnwrappedTextWidth(node) {
        if (!node) return 0;
        try {
            if (!textMeasureContext) {
                const canvas = document.createElement("canvas");
                textMeasureContext = canvas.getContext("2d");
            }
            if (!textMeasureContext) return 0;
            const style = getComputedStyle(node);
            textMeasureContext.font = style.font || [
                style.fontStyle,
                style.fontWeight,
                style.fontSize,
                style.fontFamily
            ].filter(Boolean).join(" ");
            return textMeasureContext.measureText(
                normalizeText(node.textContent)
            ).width;
        } catch (_) {
            return 0;
        }
    }

    function syncTechnicalCardWidth(card) {
        if (!card) return;
        const mode = card.getAttribute("data-tech-spec-render-mode");
        const standardWidth = resolvedStandardCardWidth(card);
        const viewportWidth = Math.max(
            160,
            document.documentElement.clientWidth || window.innerWidth || standardWidth
        );
        const scroller = card.closest(".emby-scroller");
        const scrollerWidth = scroller && scroller.clientWidth > 0
            ? scroller.clientWidth
            : viewportWidth;
        const availableWidth = Math.max(
            160,
            Math.min(viewportWidth - 48, scrollerWidth)
        );
        const minWidth = Math.min(standardWidth, availableWidth);
        const maxWidth = Math.max(
            minWidth,
            Math.min(standardWidth * 2, availableWidth)
        );
        setOwnedHeightStyle(
            card,
            card,
            "--itm-standard-card-width",
            minWidth + "px"
        );
        setOwnedHeightStyle(
            card,
            card,
            "--itm-wide-card-max-width",
            maxWidth + "px"
        );

        if (mode === "native-card") {
            setOwnedHeightStyle(card, card, "width", minWidth + "px");
            setOwnedHeightStyle(card, card, "min-width", minWidth + "px");
            setOwnedHeightStyle(card, card, "max-width", minWidth + "px");
            return;
        }
        if (mode !== "wide-card") return;

        const footer = card.querySelector(".mediaStreamInnerCardFooter");
        if (!footer) return;

        const cardRect = card.getBoundingClientRect();
        const horizontalChrome = Math.max(
            0,
            cardRect.width - footer.clientWidth
        );
        let preferredContentWidth = 0;
        for (const row of card.querySelectorAll(".mediaStreamAttribute")) {
            const label = row.querySelector(".mediaInfoAttributeLabel");
            const valueLines = row.querySelectorAll(".itm-tech-spec-value-line");
            const labelWidth = label ? label.getBoundingClientRect().width : 0;
            let valueWidth = 0;
            for (const line of valueLines) {
                valueWidth = Math.max(valueWidth, measureUnwrappedTextWidth(line));
            }
            const rowStyle = getComputedStyle(row);
            const labelStyle = label ? getComputedStyle(label) : null;
            const valueHost = row.querySelector(".mediaInfoAttributeValue");
            const valueStyle = valueHost ? getComputedStyle(valueHost) : null;
            const gap = (parseFloat(rowStyle.columnGap) || 0) +
                (labelStyle ? parseFloat(labelStyle.marginRight) || 0 : 0) +
                (valueStyle ? parseFloat(valueStyle.marginLeft) || 0 : 0);
            preferredContentWidth = Math.max(
                preferredContentWidth,
                labelWidth + gap + valueWidth
            );
        }
        const preferredWidth = Math.ceil(horizontalChrome + preferredContentWidth);
        const targetWidth = Math.min(
            maxWidth,
            Math.max(minWidth, preferredWidth || minWidth)
        );
        setOwnedHeightStyle(card, card, "width", targetWidth + "px");
    }

    function technicalCardMeaningfulBottom(card) {
        if (!card) return 0;
        let bottom = 0;
        for (const node of card.querySelectorAll(
            ".mediaStreamInnerCardFooter-cardText, .mediaStreamAttribute, " +
            ".itm-tech-spec-value-line"
        )) {
            if (!node.textContent || !normalizeText(node.textContent)) continue;
            const rect = node.getBoundingClientRect();
            if (rect.width > 0 && rect.height > 0) {
                bottom = Math.max(bottom, rect.bottom);
            }
        }
        return bottom;
    }

    function technicalCardLineHeight(card) {
        if (!card) return 20;
        let lastLine = null;
        let lastBottom = 0;
        for (const line of card.querySelectorAll(".itm-tech-spec-value-line")) {
            const rect = line.getBoundingClientRect();
            if (rect.height > 0 && rect.bottom >= lastBottom) {
                lastLine = line;
                lastBottom = rect.bottom;
            }
        }
        const sample = lastLine || card.querySelector(
            ".mediaInfoAttributeValue, .mediaStreamAttribute"
        );
        if (!sample) return 20;
        const style = getComputedStyle(sample);
        const lineHeight = parseFloat(style.lineHeight);
        if (lineHeight > 0) return Math.ceil(lineHeight);
        const fontSize = parseFloat(style.fontSize) || 14;
        return Math.ceil(fontSize * 1.45);
    }

    function technicalCardBottomClearance(card, footer) {
        const footerStyle = footer ? getComputedStyle(footer) : null;
        const nativePadding = footerStyle
            ? parseFloat(footerStyle.paddingBottom) || 0
            : 0;
        return Math.ceil(Math.max(nativePadding, technicalCardLineHeight(card)));
    }

    function technicalCardRequiredFooterHeight(card, footer) {
        if (!card || !footer) return 0;
        const footerRect = footer.getBoundingClientRect();
        const meaningfulBottom = technicalCardMeaningfulBottom(card);
        if (!meaningfulBottom || !footerRect.height) return 0;
        const style = getComputedStyle(footer);
        const borderBottom = parseFloat(style.borderBottomWidth) || 0;
        return Math.ceil(
            Math.max(0, meaningfulBottom - footerRect.top) +
            technicalCardBottomClearance(card, footer) + borderBottom
        );
    }

    function refreshNativeCardBaseline(card) {
        if (!card || card.getAttribute("data-tech-spec-render-mode") !== "native-card") {
            return null;
        }
        const liveReference = findLiveNativeReferenceCard(card);
        const current = cardLayoutBaselines.get(card) || null;
        const liveWidth = liveReference
            ? Math.max(0, liveReference.getBoundingClientRect().width || liveReference.clientWidth || 0)
            : 0;
        if (current && current.card > 0) {
            if (!liveWidth || !current.width || Math.abs(liveWidth - current.width) < 1) {
                return current;
            }
            // The native media row can stretch every card to the tallest item.
            // Reading its height after our card was inserted would feed our own
            // height back into the baseline. Scale the pre-insertion native
            // geometry by the live width instead, which also follows responsive
            // Emby breakpoints without introducing cumulative growth.
            const ratio = liveWidth / current.width;
            const scaled = {
                width: liveWidth,
                card: current.card * ratio,
                box: current.box * ratio,
                content: current.content * ratio,
                footer: current.footer * ratio
            };
            cardLayoutBaselines.set(card, scaled);
            return scaled;
        }
        const measured = captureNativeCardBaseline(liveReference);
        if (measured && measured.card > 0) {
            cardLayoutBaselines.set(card, measured);
            return measured;
        }
        return current;
    }

    function requiredNodeHeight(node, desiredBottom, baselineHeight) {
        if (!node || !desiredBottom) return Math.max(0, baselineHeight || 0);
        const rect = node.getBoundingClientRect();
        return Math.ceil(Math.max(
            baselineHeight || 0,
            desiredBottom - rect.top
        ));
    }

    function syncTechnicalCardHeight(card) {
        if (!card || !card.style) return;
        setOwnedHeightStyle(card, card, "align-self", "flex-start");
        syncTechnicalCardWidth(card);

        const footer = card.querySelector(".mediaStreamInnerCardFooter");
        if (!footer) return;
        const box = card.querySelector(".cardBox");
        const content = card.querySelector(".cardContent");
        const naturalNodes = [footer, content, box, card].filter(Boolean);
        for (const node of naturalNodes) {
            setOwnedHeightStyle(card, node, "height", "auto");
            setOwnedHeightStyle(card, node, "max-height", "none");
            setOwnedHeightStyle(card, node, "box-sizing", "border-box");
        }
        if (box) {
            setOwnedHeightStyle(card, box, "max-width", "100%");
        }
        if (content) {
            setOwnedHeightStyle(card, content, "width", "100%");
            setOwnedHeightStyle(card, content, "max-width", "100%");
            setOwnedHeightStyle(card, content, "min-width", "0px");
            setOwnedHeightStyle(card, content, "aspect-ratio", "auto");
            setOwnedHeightStyle(card, content, "display", "block");
            setOwnedHeightStyle(card, content, "overflow", "hidden");
        }
        setOwnedHeightStyle(card, footer, "width", "100%");
        setOwnedHeightStyle(card, footer, "max-width", "100%");
        setOwnedHeightStyle(card, footer, "min-width", "0px");
        setOwnedHeightStyle(card, footer, "display", "block");

        // Natural block sizing now owns the height. Compensate only when an
        // Emby theme still clips the last meaningful row. Measuring the last
        // row (rather than a stretched footer) avoids the ISO empty-tail bug.
        const baseline = refreshNativeCardBaseline(card) || {};
        const meaningfulBottom = technicalCardMeaningfulBottom(card);
        const desiredBottom = meaningfulBottom +
            technicalCardBottomClearance(card, footer);
        const requiredFooterHeight = technicalCardRequiredFooterHeight(card, footer);

        setOwnedHeightStyle(
            card,
            footer,
            "min-height",
            Math.max(requiredFooterHeight, baseline.footer || 0) + "px"
        );
        for (const [node, baselineHeight] of [
            [content, baseline.content],
            [box, baseline.box],
            [card, baseline.card]
        ]) {
            if (!node) continue;
            setOwnedHeightStyle(
                card,
                node,
                "min-height",
                requiredNodeHeight(node, desiredBottom, baselineHeight) + "px"
            );
        }
        cardLayoutSignatures.set(card, technicalCardLayoutInputSignature(card));
    }

    function scheduleTechnicalCardHeightSync(card) {
        if (!card || cardLayoutSyncPending.has(card)) return;
        cardLayoutSyncPending.add(card);
        requestAnimationFrame(() => {
            cardLayoutSyncPending.delete(card);
            if (!card.isConnected) return;
            syncTechnicalCardHeight(card);
            updateTechnicalCardLayoutDebug(card);
        });
    }

    function technicalCardLayoutInputSignature(card) {
        if (!card || !card.isConnected) return "";
        const reference = findLiveNativeReferenceCard(card);
        const referenceWidth = reference
            ? Math.round(reference.getBoundingClientRect().width)
            : 0;
        const cardRect = card.getBoundingClientRect();
        const meaningfulBottom = technicalCardMeaningfulBottom(card);
        return [
            document.documentElement.clientWidth || window.innerWidth || 0,
            referenceWidth,
            Math.round(cardRect.width),
            Math.round(meaningfulBottom - cardRect.top)
        ].join("|");
    }

    function observeTechnicalCardHeight(card) {
        if (!card || cardHeightObservers.has(card) ||
            typeof ResizeObserver !== "function") return;
        const footer = card.querySelector(".mediaStreamInnerCardFooter");
        if (!footer) return;
        let pending = false;
        const resizeObserver = new ResizeObserver(() => {
            if (!card.isConnected) {
                disconnectTechnicalCardHeightObserver(card);
                return;
            }
            if (pending) return;
            if (cardLayoutSignatures.get(card) ===
                technicalCardLayoutInputSignature(card)) return;
            pending = true;
            requestAnimationFrame(() => {
                pending = false;
                if (card.isConnected) {
                    syncTechnicalCardHeight(card);
                    updateTechnicalCardLayoutDebug(card);
                }
            });
        });
        resizeObserver.observe(footer);
        resizeObserver.observe(document.documentElement);
        cardHeightObservers.set(card, resizeObserver);
    }

    function disconnectTechnicalCardHeightObserver(card) {
        const resizeObserver = cardHeightObservers.get(card);
        if (!resizeObserver) return;
        resizeObserver.disconnect();
        cardHeightObservers.delete(card);
        cardLayoutSignatures.delete(card);
    }

    function captureNativeSiblingGeometry(container) {
        if (!container) return null;
        const cards = [...container.children].filter(node => (
            node.matches && node.matches(".card") &&
            !node.hasAttribute("data-tech-spec-card")
        ));
        return {
            viewportWidth: document.documentElement.clientWidth || window.innerWidth || 0,
            cards: cards.map(card => ({
                card,
                width: card.getBoundingClientRect().width,
                flexBasis: getComputedStyle(card).flexBasis
            }))
        };
    }

    function nativeSiblingGeometryChanged(snapshot) {
        if (!snapshot) return false;
        const viewportWidth = document.documentElement.clientWidth || window.innerWidth || 0;
        if (Math.abs(viewportWidth - snapshot.viewportWidth) > 1) return false;
        return snapshot.cards.some(before => {
            if (!before.card || !before.card.isConnected) return false;
            const afterWidth = before.card.getBoundingClientRect().width;
            const afterFlexBasis = getComputedStyle(before.card).flexBasis;
            return Math.abs(afterWidth - before.width) > 2 ||
                afterFlexBasis !== before.flexBasis;
        });
    }

    function createIsolatedTargetBesideNativeContainer(nativeContainer) {
        if (!nativeContainer) return null;
        const anchor = nativeContainer.closest(
            ".verticalSection.audioVideoMediaInfo, .verticalSection, .mediaSources"
        );
        if (!anchor || !anchor.parentNode) return null;
        const generated = createNativeMediaHost();
        generated.host.setAttribute(
            "data-tech-spec-placement",
            "native-sibling-geometry-guard"
        );
        anchor.parentNode.insertBefore(generated.host, anchor);
        return generated;
    }

    function protectNativeSiblingGeometry(card, nativeContainer, snapshot) {
        if (!card || !nativeContainer || !snapshot) return;
        requestAnimationFrame(() => requestAnimationFrame(() => {
            if (!card.isConnected || !nativeContainer.isConnected) return;
            const changed = nativeSiblingGeometryChanged(snapshot);
            if (window.__technicalSpecsDebug) {
                window.__technicalSpecsDebug.nativeSiblingGeometryPreserved = !changed;
            }
            if (!changed) return;

            const isolated = createIsolatedTargetBesideNativeContainer(
                nativeContainer
            );
            if (!isolated) {
                if (window.__technicalSpecsDebug) {
                    window.__technicalSpecsDebug.nativeIsolationRequired = true;
                    window.__technicalSpecsDebug.nativeIsolationApplied = false;
                }
                return;
            }
            isolated.container.appendChild(card);
            card.setAttribute(
                "data-tech-spec-placement",
                "native-sibling-geometry-guard"
            );
            cardLayoutSignatures.delete(card);
            syncTechnicalCardHeight(card);
            updateTechnicalCardLayoutDebug(card);
            if (window.__technicalSpecsDebug) {
                window.__technicalSpecsDebug.nativeIsolationRequired = true;
                window.__technicalSpecsDebug.nativeIsolationApplied = true;
            }
        }));
    }

    function technicalCardContainsContent(card) {
        const footer = card && card.querySelector(".mediaStreamInnerCardFooter");
        const content = card && card.querySelector(".cardContent");
        if (!card || !footer || !content) return false;
        try {
            const cardRect = card.getBoundingClientRect();
            const contentRect = content.getBoundingClientRect();
            const box = card.querySelector(".cardBox");
            const meaningfulBottom = technicalCardMeaningfulBottom(card);
            if (!meaningfulBottom) return false;
            const bottomClearance = technicalCardBottomClearance(card, footer);
            const baseline = cardLayoutBaselines.get(card) || {};
            const bottomChrome = [footer, content, box, card].filter(Boolean).reduce(
                (total, node) => {
                    const style = getComputedStyle(node);
                    return total +
                        (parseFloat(style.marginBottom) || 0) +
                        (parseFloat(style.paddingBottom) || 0) +
                        (parseFloat(style.borderBottomWidth) || 0);
                },
                0
            );
            const normalTrailingSpace = Math.min(
                96,
                Math.max(
                    bottomClearance + 12,
                    Math.ceil(bottomChrome + bottomClearance)
                )
            );
            const baselineTrailingSpace = baseline.card
                ? Math.max(
                    0,
                    baseline.card - (meaningfulBottom - cardRect.top)
                )
                : 0;
            const allowedTrailingSpace = Math.max(
                normalTrailingSpace,
                Math.ceil(baselineTrailingSpace + 2)
            );
            const trailingSpace = cardRect.bottom - meaningfulBottom;
            const shellsFit = [box, content, footer].filter(Boolean).every(node => {
                const rect = node.getBoundingClientRect();
                return (
                    node.scrollWidth <= node.clientWidth + 2 &&
                    rect.right <= cardRect.right + 2
                );
            });
            const valuesFit = [...card.querySelectorAll(
                ".mediaStreamInnerCardFooter-cardText, " +
                ".mediaInfoAttributeValue, .itm-tech-spec-value-line"
            )].every(node => {
                const rect = node.getBoundingClientRect();
                return (
                    node.scrollWidth <= node.clientWidth + 2 &&
                    rect.right <= contentRect.right + 2 &&
                    rect.right <= cardRect.right + 2
                );
            });
            return (
                footer.scrollHeight <= footer.clientHeight + 2 &&
                footer.scrollWidth <= footer.clientWidth + 2 &&
                meaningfulBottom <= contentRect.bottom + 2 &&
                meaningfulBottom <= cardRect.bottom + 2 &&
                trailingSpace >= bottomClearance - 2 &&
                trailingSpace <= allowedTrailingSpace &&
                shellsFit &&
                valuesFit
            );
        } catch (_) {
            return false;
        }
    }

    function isRenderedCardVisible(card) {
        if (!card || !isInVisibleTree(card)) return false;
        try {
            syncTechnicalCardHeight(card);
            const rect = card.getBoundingClientRect();
            return rect.width > 1 && rect.height > 1;
        } catch (_) {
            return false;
        }
    }

    function updateTechnicalCardLayoutDebug(card) {
        if (!card || !card.isConnected || !window.__technicalSpecsDebug) return;
        const layoutVerified = technicalCardContainsContent(card);
        window.__technicalSpecsDebug.layoutVerified = layoutVerified;
        window.__technicalSpecsDebug.layoutPending = !layoutVerified;
        window.__technicalSpecsDebug.renderWidth = Math.round(
            card.getBoundingClientRect().width
        );
    }

    function removeOldTechnicalCards(options = {}) {
        const keepHost = options.keepHost || null;

        // Broad selector: also removes cards left by older script versions
        // (any data-tech-spec-card value) and legacy classed shells, so a
        // stale cached copy can never render alongside the current card.
        document
            .querySelectorAll("[data-tech-spec-card], .itm-tech-card, .techSpecsFallbackShell")
            .forEach(card => {
                disconnectTechnicalCardHeightObserver(card);
                resetTechnicalCardHeight(card);
                card.remove();
            });

        document
            .querySelectorAll(NATIVE_HOST_SELECTOR)
            .forEach(host => {
                if (host !== keepHost) host.remove();
            });
    }

    function currentCardMatches(container, key, mode) {
        if (!container || !key) return false;

        const existing = container.querySelector(TECH_CARD_SELECTOR);
        if (!existing) return false;

        return (
            lastRenderKey === key &&
            existing.getAttribute("data-tech-spec-render-mode") === mode &&
            existing.getAttribute("data-tech-spec-render-key") === key
        );
    }

    function findMatchingTechnicalCard(root, key, mode) {
        if (!root || !key || lastRenderKey !== key) return null;
        return [...root.querySelectorAll(TECH_CARD_SELECTOR)].find(card => (
            card.getAttribute("data-tech-spec-render-mode") === mode &&
            card.getAttribute("data-tech-spec-render-key") === key &&
            isInVisibleTree(card)
        )) || null;
    }

    async function render(requestId) {
        const locationKey = getLocationKey();

        if (!renderRequestStillCurrent(requestId, locationKey)) return "stale";
        if (!await refreshRuntimeLease(false)) {
            removeOldTechnicalCards();
            lastRenderKey = "";
            lastRenderOutcome = "service-stopped";
            return "service-stopped";
        }
        if (!renderRequestStillCurrent(requestId, locationKey)) return "stale";

        // Emby route formats vary between desktop and server builds.  An item
        // id lets us ask ApiClient for extra detail, but it is not required to
        // render a verified Movie/Series record: the visible detail page, IMDb
        // provider id and public card index form a safe fallback identity.
        const itemId = getItemId();
        let item = null;
        let imdb = "";
        let identitySource = "";
        let itemType = "";

        const detailRoot = findVisibleDetailRoot();
        if (!detailRoot) {
            removeOldTechnicalCards();
            lastRenderKey = "";
            lastRenderOutcome = "retry";
            return debugFailure("visible-detail-root-not-ready", {
                itemId,
                locationKey,
                stage: "detail-root"
            });
        }

        item = itemId ? await getCurrentItem(itemId) : null;
        if (!renderRequestStillCurrent(requestId, locationKey)) return "stale";
        itemType = normalizeItemType(item && item.Type) || getVisibleItemType(detailRoot);

        // A known Episode/Season or unsupported page is rejected before any
        // provider-link fallback. This prevents a Series IMDb link displayed
        // on an Episode page from causing a card to appear on the wrong page.
        if (itemType && CARD_SUPPRESSED_TYPES.has(itemType)) {
            removeOldTechnicalCards();
            lastRenderKey = "";
            lastRenderOutcome = "suppressed";
            window.__technicalSpecsDebug = {
                version: WEB_CARD_VERSION,
                itemId,
                itemType,
                locationKey,
                rendered: false,
                suppressed: true,
                reason: "tv-series-home-only"
            };
            return "suppressed";
        }

        if (itemType && !CARD_ELIGIBLE_TYPES.has(itemType)) {
            removeOldTechnicalCards();
            lastRenderKey = "";
            lastRenderOutcome = "suppressed";
            return debugFailure("unsupported-item-type", {
                itemId, itemType, locationKey, suppressed: true
            });
        }

        const providerIds = item && item.ProviderIds || {};
        imdb = normalizeText(
            providerIds.Imdb || providerIds.IMDb || providerIds.imdb
        ).toLowerCase();
        if (imdb) identitySource = "route-api";

        // DOM provider IDs are a fallback only after current route identity and
        // item type have been confirmed through Emby's API.
        if (!imdb) {
            imdb = getImdbFromDom(detailRoot);
            if (imdb) identitySource = "visible-dom";
        }

        if (!renderRequestStillCurrent(requestId, locationKey)) return "stale";

        if (!imdb) {
            removeOldTechnicalCards();
            lastRenderKey = "";
            lastRenderOutcome = "retry";
            window.__technicalSpecsDebug = {
                version: WEB_CARD_VERSION,
                itemId,
                itemType,
                locationKey,
                identitySource: "pending",
                dataFound: false,
                rendered: false,
                retryReason: "imdb-id-not-ready"
            };
            return "imdb-id-not-ready";
        }

        let database = await getTechDatabase(false);
        if (!renderRequestStillCurrent(requestId, locationKey)) return "stale";

        if (!database || !database.items) {
            removeOldTechnicalCards();
            lastRenderKey = "";
            lastRenderOutcome = "retry";
            return debugFailure("database-not-ready", {
                imdb, itemId, itemType, locationKey, identitySource
            });
        }

        const indexedItemType = getIndexedItemType(database, imdb);
        if (!itemType) itemType = indexedItemType;
        if (!itemType) {
            removeOldTechnicalCards();
            lastRenderKey = "";
            lastRenderOutcome = "retry";
            window.__technicalSpecsDebug = {
                version: WEB_CARD_VERSION,
                imdb,
                itemId,
                locationKey,
                identitySource,
                dataFound: false,
                rendered: false,
                retryReason: "item-type-not-in-public-index"
            };
            return "item-type-not-in-public-index";
        }
        if (indexedItemType && indexedItemType !== itemType) {
            removeOldTechnicalCards();
            lastRenderKey = "";
            lastRenderOutcome = "retry";
            return debugFailure("route-index-type-mismatch", {
                imdb, itemId, itemType, indexedItemType, locationKey, identitySource
            });
        }
        if (CARD_SUPPRESSED_TYPES.has(itemType) || !CARD_ELIGIBLE_TYPES.has(itemType)) {
            removeOldTechnicalCards();
            lastRenderKey = "";
            lastRenderOutcome = "suppressed";
            return "suppressed";
        }

        let specs =
            database.items[imdb] ||
            database.items[imdb.toLowerCase()] ||
            database.items[imdb.toUpperCase()];

        if (!specs) {
            // A Windows index rebuild can finish just after the page is opened.
            // Bypass the short memory cache once before entering the retry loop.
            const fresh = await getTechDatabase(true);
            if (!renderRequestStillCurrent(requestId, locationKey)) return "stale";
            if (fresh && fresh.items) {
                database = fresh;
                specs =
                    database.items[imdb] ||
                    database.items[imdb.toLowerCase()] ||
                    database.items[imdb.toUpperCase()];
            }
        }

        if (!specs) {
            removeOldTechnicalCards();
            lastRenderKey = "";
            lastRenderOutcome = "retry";
            window.__technicalSpecsDebug = {
                version: WEB_CARD_VERSION,
                imdb,
                itemId,
                itemType,
                locationKey,
                identitySource,
                dataFound: false,
                rendered: false,
                retryReason: "item-not-in-index-yet"
            };
            return "item-not-in-index-yet";
        }

        const zh = uiIsChinese();
        const key = [
            itemId || imdb,
            itemType || "unknown",
            imdb,
            zh ? "zh" : "en",
            database.generatedAt || ""
        ].join("|");

        if (!renderRequestStillCurrent(requestId, locationKey)) return "stale";

        // A normal movie can clone the visible native Video card directly.
        if (itemType !== "Series") {
            const existingCard = findMatchingTechnicalCard(
                detailRoot,
                key,
                "native-card"
            );
            if (existingCard) {
                syncTechnicalCardHeight(existingCard);
                updateTechnicalCardLayoutDebug(existingCard);
                lastRenderOutcome = "current";
                return "current";
            }

            const native = findNativeVideoCard(detailRoot);
            if (native) {
                removeOldTechnicalCards();

                const card = buildTechnicalCard(
                    specs,
                    zh,
                    "native-card",
                    measureNativeCardWidth(native.card),
                    native
                );
                if (!card) return debugFailure("empty-specs", {
                    imdb, itemId, itemType, locationKey, dataFound: true
                });
                if (!renderRequestStillCurrent(requestId, locationKey)) return "stale";

                card.setAttribute("data-tech-spec-render-key", key);
                card.setAttribute("data-tech-spec-item-id", itemId || "");
                card.setAttribute("data-tech-spec-item-type", itemType || "");
                card.setAttribute("data-tech-spec-imdb-id", imdb);
                const nativeSiblingGeometry = captureNativeSiblingGeometry(
                    native.container
                );
                native.container.insertBefore(
                    card,
                    native.container.firstElementChild
                );
                syncTechnicalCardHeight(card);
                observeTechnicalCardHeight(card);
                scheduleTechnicalCardHeightSync(card);

                if (!isRenderedCardVisible(card)) {
                    disconnectTechnicalCardHeightObserver(card);
                    card.remove();
                    lastRenderKey = "";
                    lastRenderOutcome = "retry";
                    return debugFailure("native-card-not-visible", {
                        imdb, itemId, itemType, locationKey,
                        dataFound: true, nativeCardFound: true
                    });
                }

                lastRenderKey = key;
                lastRenderOutcome = "rendered";
                const layoutVerified = technicalCardContainsContent(card);
                setTechnicalSpecsDebug({
                    imdb,
                    itemId,
                    itemType,
                    locationKey,
                    identitySource,
                    dataFound: true,
                    nativeCardFound: true,
                    nativeShell: "cloned-live-video-card",
                    visible: true,
                    layoutVerified,
                    layoutPending: !layoutVerified,
                    rendered: true
                });
                protectNativeSiblingGeometry(
                    card,
                    native.container,
                    nativeSiblingGeometry
                );
                return "rendered";
            }
        }

        // ISO / BDMV / Series: place one responsive card inside the current
        // detail page's media scroller hierarchy.
        const target = getOrCreateNativeTarget(detailRoot);
        if (!target || !target.container) {
            lastRenderOutcome = "retry";
            window.__technicalSpecsDebug = {
                version: WEB_CARD_VERSION,
                imdb,
                itemId,
                itemType,
                locationKey,
                identitySource,
                dataFound: true,
                nativeCardFound: false,
                nativeHierarchyUsed: true,
                nativeTargetFound: false,
                rendered: false,
                retryReason: "native-target-not-ready"
            };
            return "native-target-not-ready";
        }

        if (currentCardMatches(target.container, key, "wide-card")) {
            const existing = target.container.querySelector(TECH_CARD_SELECTOR);
            if (!isRenderedCardVisible(existing)) {
                removeOldTechnicalCards();
                lastRenderKey = "";
                lastRenderOutcome = "retry";
                return debugFailure("existing-card-not-visible", {
                    imdb, itemId, itemType, locationKey, dataFound: true,
                    nativePlacement: target.placement
                });
            }
            syncTechnicalCardHeight(
                existing
            );
            updateTechnicalCardLayoutDebug(existing);
            lastRenderOutcome = "current";
            return "current";
        }

        removeOldTechnicalCards({
            keepHost: target.createdHost
                ? (target.host || target.container.closest(NATIVE_HOST_SELECTOR))
                : null
        });

        const nativeTemplate = findAnyNativeMediaCard(detailRoot);
        const card = buildTechnicalCard(
            specs,
            zh,
            "wide-card",
            null,
            nativeTemplate
        );
        if (!card) return debugFailure("empty-specs", {
            imdb, itemId, itemType, locationKey, dataFound: true
        });
        if (!renderRequestStillCurrent(requestId, locationKey)) return "stale";

        card.setAttribute("data-tech-spec-render-key", key);
        card.setAttribute("data-tech-spec-item-id", itemId || "");
        card.setAttribute("data-tech-spec-item-type", itemType || "");
        card.setAttribute("data-tech-spec-imdb-id", imdb);
        target.container.insertBefore(
            card,
            target.container.firstElementChild
        );
        syncTechnicalCardHeight(card);
        observeTechnicalCardHeight(card);
        scheduleTechnicalCardHeightSync(card);

        if (!isRenderedCardVisible(card)) {
            disconnectTechnicalCardHeightObserver(card);
            card.remove();
            if (target.createdHost && target.host) target.host.remove();
            lastRenderKey = "";
            lastRenderOutcome = "retry";
            return debugFailure("native-target-not-visible", {
                imdb, itemId, itemType, locationKey, identitySource,
                dataFound: true, nativePlacement: target.placement
            });
        }

        lastRenderKey = key;
        lastRenderOutcome = "rendered";
        const layoutVerified = technicalCardContainsContent(card);
        setTechnicalSpecsDebug({
            imdb,
            itemId,
            itemType,
            locationKey,
            identitySource,
            dataFound: true,
            nativeCardFound: false,
            nativeTargetFound: true,
            nativePlacement: target.placement,
            nativeShell: nativeTemplate
                ? "cloned-live-media-card"
                : "captured-emby-4.9.5-shell",
            visible: true,
            layoutVerified,
            layoutPending: !layoutVerified,
            rendered: true
        });
        return "rendered";
    }

    function clearRetryTimer(resetAttempt = false) {
        if (retryTimer) {
            clearTimeout(retryTimer);
            retryTimer = 0;
        }
        if (resetAttempt) retryAttempt = 0;
    }

    function queueRetry(requestId, locationKey, reason) {
        if (!renderRequestStillCurrent(requestId, locationKey)) return;
        if (retryTimer) return;
        if (retryAttempt >= RETRY_DELAYS_MS.length) return;

        const delay = RETRY_DELAYS_MS[retryAttempt++];
        retryTimer = setTimeout(() => {
            retryTimer = 0;
            if (!renderRequestStillCurrent(requestId, locationKey)) return;
            scheduleRender(`retry:${reason || "pending"}`, 0);
        }, delay);
    }

    async function runScheduledRender() {
        renderTimer = 0;

        if (renderInFlight) {
            pendingRender = true;
            return;
        }

        renderInFlight = true;
        pendingRender = false;

        const requestId = renderRequestId;
        const locationKey = getLocationKey();
        let result = "retry";

        try {
            result = await render(requestId);
        } catch (error) {
            result = "render-exception";
            lastRenderOutcome = "retry";
            debugFailure("render-exception", {
                locationKey,
                error: {
                    name: String(error && error.name || "Error"),
                    message: String(error && error.message || error || "Unknown render error"),
                    stack: String(error && error.stack || "").split("\n").slice(0, 6).join("\n")
                }
            });
        } finally {
            renderInFlight = false;
        }

        // A route change during ApiClient/fetch invalidates the old result.
        // The route-change scheduler already queued the new item.
        if (!renderRequestStillCurrent(requestId, locationKey)) {
            scheduleRender("stale-completion", 0);
            return;
        }

        if (
            result === "rendered" || result === "current" ||
            result === "suppressed" || result === "empty-specs"
        ) {
            clearRetryTimer(true);
        } else {
            queueRetry(requestId, locationKey, result);
        }

        if (pendingRender && !renderTimer) {
            scheduleRender("pending-dom", 60);
        }
    }

    function scheduleRender(reason = "event", delay = 80) {
        const locationKey = getLocationKey();
        const routeItemId = getRouteItemId();
        const routeChanged = lastLocationKey !== locationKey;
        const itemChanged = Boolean(
            routeItemId && lastRouteItemId &&
            routeItemId.toLowerCase() !== lastRouteItemId.toLowerCase()
        );

        // renderRequestId is a ROUTE epoch, not a MutationObserver debounce id.
        // DOM churn on the same movie must not continually invalidate an
        // ApiClient/index request that is already in flight.
        if (!lastLocationKey || routeChanged || itemChanged) {
            lastLocationKey = locationKey;
            lastRouteItemId = routeItemId;
            renderRequestId++;
            clearRetryTimer(true);
            pendingRender = true;

            if (routeChanged || itemChanged) {
                removeOldTechnicalCards();
                lastRenderKey = "";
                lastRenderOutcome = "pending";
            }

            if (renderTimer) {
                clearTimeout(renderTimer);
                renderTimer = 0;
            }
        }

        // Leading-edge coalescing: mutation storms no longer keep pushing the
        // render timer into the future forever. One attempt is guaranteed to
        // run while Emby is still rebuilding the detail view.
        pendingRender = true;
        if (renderTimer || renderInFlight) return;

        renderTimer = setTimeout(runScheduledRender, Math.max(0, delay));
    }

    function installHistoryHooks() {
        for (const name of ["pushState", "replaceState"]) {
            try {
                const original = history[name];
                if (typeof original !== "function") continue;
                if (original.__technicalSpecsWrapped) continue;

                const wrapped = function (...args) {
                    const result = original.apply(this, args);
                    scheduleRender(`history:${name}`, 0);
                    return result;
                };
                wrapped.__technicalSpecsWrapped = true;
                history[name] = wrapped;
                historyRestores.push(() => {
                    if (history[name] === wrapped) history[name] = original;
                });
            } catch (_) {}
        }
    }

    function listen(target, type, handler, options) {
        target.addEventListener(type, handler, options);
        cleanupCallbacks.push(() => target.removeEventListener(type, handler, options));
    }

    function managerOwnsMutationNode(node) {
        if (!node || node.nodeType !== Node.ELEMENT_NODE) return false;
        return Boolean(
            node.matches(TECH_CARD_SELECTOR + "," + NATIVE_HOST_SELECTOR) ||
            node.closest(TECH_CARD_SELECTOR + "," + NATIVE_HOST_SELECTOR)
        );
    }

    function mutationTouchesRenderSurface(record) {
        const changed = [...record.addedNodes, ...record.removedNodes]
            .filter(node => node.nodeType === Node.ELEMENT_NODE);
        if (changed.length && changed.every(managerOwnsMutationNode)) {
            return false;
        }

        const directSurface =
            ".detailMediaStreamsItemsContainer, .mediaSources, " +
            ".verticalSection.audioVideoMediaInfo";
        const mountedSurface =
            ".itemView, .itemDetailPage, .detailPage, " + directSurface;
        const target = record.target && record.target.nodeType === Node.ELEMENT_NODE
            ? record.target
            : null;
        if (target && !managerOwnsMutationNode(target) &&
            (target.matches(directSurface) || target.closest(directSurface))) {
            return true;
        }
        return changed.some(node => (
            !managerOwnsMutationNode(node) &&
            (node.matches(mountedSurface) || node.querySelector(mountedSurface))
        ));
    }

    /*
     * Emby item.js rebuilds .mediaSources while navigating or switching media
     * versions. Observe structural DOM changes, but coalesce them instead of
     * debouncing forever. History hooks catch SPA route changes immediately.
     */
    observer = new MutationObserver(records => {
        if (records.some(mutationTouchesRenderSurface)) {
            scheduleRender("relevant-mutation");
        }
    });

    observer.observe(document.documentElement, {
        subtree: true,
        childList: true
    });

    installHistoryHooks();

    listen(window, "hashchange", () => scheduleRender("hashchange", 0));
    listen(window, "popstate", () => scheduleRender("popstate", 0));
    listen(window, "pageshow", () => scheduleRender("pageshow", 0));
    listen(window, "focus", () => scheduleRender("window-focus", 0));
    listen(window, "resize", () => {
        document.querySelectorAll(TECH_CARD_SELECTOR)
            .forEach(scheduleTechnicalCardHeightSync);
        scheduleRender("window-resize", 80);
    }, { passive: true });
    listen(document, "viewshow", () => scheduleRender("viewshow", 0), true);
    listen(document, "itemshow", () => scheduleRender("itemshow", 0), true);
    listen(document, "visibilitychange", () => {
        if (!document.hidden) scheduleRender("visibility-resume", 0);
    }, true);

    leaseInterval = setInterval(async () => {
        const allowed = await enforceRuntimeLease("lease-poll");
        if (allowed && !document.querySelector(TECH_CARD_SELECTOR)) {
            scheduleRender("lease-poll", 0);
        }
    }, CARD_LEASE_POLL_MS);

    // Lightweight safety net: detect route/item changes and a card left over
    // from a different item without restoring the historical strict detail-root gate.
    watchdogInterval = setInterval(() => {
        const routeItemId = getRouteItemId();
        if (getLocationKey() !== lastLocationKey) {
            scheduleRender("route-watchdog", 0);
            return;
        }
        if (
            routeItemId && lastRouteItemId &&
            routeItemId.toLowerCase() !== lastRouteItemId.toLowerCase()
        ) {
            scheduleRender("item-watchdog", 0);
            return;
        }
        if (routeItemId) {
            const stale = [...document.querySelectorAll(TECH_CARD_SELECTOR)].some(card => {
                const cardItemId = String(card.getAttribute("data-tech-spec-item-id") || "");
                return cardItemId && cardItemId.toLowerCase() !== routeItemId.toLowerCase();
            });
            if (stale) {
                removeOldTechnicalCards();
                scheduleRender("stale-card-watchdog", 0);
                return;
            }
        }
        if (
            !document.querySelector(TECH_CARD_SELECTOR) &&
            (lastRenderOutcome === "rendered" || lastRenderOutcome === "current")
        ) {
            scheduleRender("card-removed-watchdog", 0);
        }
    }, 600);

    window.__itmTechCardTeardown = () => {
        renderRequestId++;
        if (renderTimer) clearTimeout(renderTimer);
        if (retryTimer) clearTimeout(retryTimer);
        if (watchdogInterval) clearInterval(watchdogInterval);
        if (leaseInterval) clearInterval(leaseInterval);
        if (observer) observer.disconnect();
        for (const cleanup of cleanupCallbacks.splice(0)) {
            try { cleanup(); } catch (_) {}
        }
        for (const restore of historyRestores.splice(0).reverse()) {
            try { restore(); } catch (_) {}
        }
        removeOldTechnicalCards();
        for (const resizeObserver of cardHeightObservers.values()) {
            resizeObserver.disconnect();
        }
        cardHeightObservers.clear();
        const style = document.getElementById("tech-spec-card-style");
        if (style) style.remove();
    };

    scheduleRender("initial", 0);

})();
