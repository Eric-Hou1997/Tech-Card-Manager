"use strict";

const fs = require("fs");
const path = require("path");
const vm = require("vm");

const html = fs.readFileSync(path.resolve(__dirname, "..", "web", "index.html"), "utf8");

function sourceOf(name) {
    const match = html.match(new RegExp(`function ${name}\\([^\\n]+`));
    if (!match) throw new Error(`missing responsive decision function: ${name}`);
    return match[0];
}

const context = {};
vm.createContext(context);
vm.runInContext([
    sourceOf("headerLayoutMode"),
    sourceOf("consoleLayoutMode"),
    sourceOf("catalogToolsNeedCompact"),
    sourceOf("catalogLayoutMode"),
    "globalThis.layoutTest={headerLayoutMode,consoleLayoutMode,catalogToolsNeedCompact,catalogLayoutMode};"
].join("\n"), context);

function assert(condition, message) {
    if (!condition) throw new Error(message);
    console.log("OK  " + message);
}

const layout = context.layoutTest;

// The values represent measured rendered widths, not language identifiers.
// A future longer locale therefore follows the same decisions automatically.
assert(layout.headerLayoutMode(1180, 430, 280, 300) === "wide", "Chinese-width header stays on one row while it fits");
assert(layout.headerLayoutMode(1280, 470, 360, 360) === "wide", "English-width header stays on one row while it fits");
assert(layout.headerLayoutMode(880, 470, 360, 360) === "status-below", "medium header keeps commands top-right and moves badges below");
assert(layout.headerLayoutMode(780, 470, 360, 360) === "stacked", "header fully stacks only when brand and commands stop fitting");
for (const widths of [[430, 280, 300], [470, 360, 360], [520, 480, 390]]) {
    const ranks = [];
    for (let container = 1600; container >= 420; container -= 2) {
        ranks.push({wide: 0, "status-below": 1, stacked: 2}[layout.headerLayoutMode(container, ...widths)]);
    }
    assert(ranks.every((rank, index) => index === 0 || rank >= ranks[index - 1]), `header states never reverse for measured widths ${widths.join("/")}`);
}
assert(html.includes(".top.layoutStatusBelow .commandActions{grid-column:2;grid-row:1;justify-self:end}"), "medium header anchors refresh and settings at the upper right");
assert(html.includes(".top.layoutStatusBelow .statusActions{grid-column:1/-1;grid-row:2;justify-self:end}"), "medium header moves the badge group to the next row");
assert(html.includes("row-gap:4px"), "two-row header uses compact vertical spacing");
assert(html.includes(".btn:hover{transform:none}"), "buttons no longer lift on hover");
assert(html.includes("measureHeaderIntrinsicWidths()") && !html.includes("brand.scrollWidth") && !html.includes("statusActions.scrollWidth") && !html.includes("commandActions.scrollWidth"), "header breakpoints use layout-independent intrinsic measurements");
assert(!html.includes(".logo{width:58px") && !html.includes("h1{font-size:23px}") && !html.includes(".titleLine h1{font-size:clamp"), "narrow breakpoint never shrinks the brand and reverses a header state");
assert(html.includes(".brand{flex:0 0 max-content;min-width:max-content;white-space:nowrap}"), "brand is an indivisible fixed-width unit");
assert(html.includes(".top.layoutStatusBelow .brand{grid-column:1;grid-row:1;justify-self:start}"), "brand grid cell never stretches beyond its fixed content width");
assert(html.includes(".logo{flex:0 0 68px;width:68px;height:68px;min-width:68px;min-height:68px;max-width:68px;max-height:68px}"), "logo cannot shrink at any responsive state");
assert(!html.includes(".logo{flex:0 0 auto}") && !html.includes(".brand{flex-basis:auto}"), "narrow rules do not override the fixed brand sizing");
assert(html.includes("h1{flex:0 0 auto;font-size:28px;line-height:1.2;white-space:nowrap}") && html.includes(".version{flex:0 0 auto;font-size:21px;line-height:1.2;white-space:nowrap}") && html.includes(".sub{font-size:15px;white-space:nowrap}"), "title, version, and subtitle keep fixed typography without splitting");
assert(html.includes("padding:clamp(19px,2.8vw,28px) clamp(13px,2.8vw,28px) 54px"), "page spacing changes continuously instead of jumping at 650px");
assert(html.includes("html{scrollbar-gutter:stable}"), "document scrollbar cannot change the responsive measurement width");

assert(layout.consoleLayoutMode(900, 300, 135, 170) === "five", "Chinese-width console keeps all five cards down to the later breakpoint");
assert(layout.consoleLayoutMode(760, 300, 150, 170) === "four", "wide console keeps four metrics together down to the later breakpoint");
assert(layout.consoleLayoutMode(690, 300, 150, 170) === "two", "narrow console switches directly to two-by-two metrics");
assert(layout.consoleLayoutMode(1200, 340, 190, 190) === "five", "English-width console keeps five cards when measured copy fits");
assert(layout.consoleLayoutMode(1000, 340, 190, 190) === "four", "English-width console delays its first fallback until needed");
assert(Array.from({length: 184}, (_, index) => 320 + index * 7).every(width =>
    ["five", "four", "two"].includes(layout.consoleLayoutMode(width, 360, 180, 220))
), "console exposes only the three intentional layout states");
assert(html.includes(".console.layoutFiveCards{grid-template-columns:"), "five-card horizontal grid remains available");
assert(html.includes(".shell{max-width:1500px;padding:clamp"), "wide Manager window exposes enough space for the five-card row");

assert(!layout.catalogToolsNeedCompact(1180, 210, 430, 220), "wide catalog tools remain on one row");
assert(layout.catalogToolsNeedCompact(720, 210, 430, 220), "catalog tabs move as one unit when all tools stop fitting");
assert(html.includes(".catalogTools.layoutCompact .search{grid-column:1}.catalogTools.layoutCompact .select{grid-column:2}"), "search and status filter always share the compact row");
assert(!html.includes(".catalogTools .search{flex-basis:100%"), "narrow layout no longer forces search onto its own row");
assert(layout.catalogLayoutMode(900) === "split" && layout.catalogLayoutMode(650) === "split", "NFO list and preview remain split well below the old shared breakpoint");
assert(layout.catalogLayoutMode(620) === "single", "NFO layout changes only when its own structural minimum stops fitting");
assert(html.includes(".catalogLayout.layoutSingleColumn{grid-template-columns:1fr}"), "NFO single-column state has its own explicit class");
assert(!html.includes("@media(max-width:980px)") && !html.includes("matchMedia('(max-width:980px)')"), "console and NFO layout no longer share the 980px breakpoint");
assert(html.includes("layout.classList.contains('layoutSingleColumn')"), "catalog height synchronization follows the NFO layout state instead of viewport width");
assert(html.includes('src="../assets/TCM_logo_letter_only.png"'), "file preview resolves the real logo instead of wrapping fallback alt text");

console.log("OK Tech Card Manager content-measured responsive layout states");
