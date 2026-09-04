"use strict";

const fs = require("fs");
const vm = require("vm");
const path = require("path");

const windowsRoot = path.resolve(__dirname, "..");
const html = fs.readFileSync(path.join(windowsRoot, "web", "index.html"), "utf8");
const scripts = [...html.matchAll(/<script(?:\s[^>]*)?>([\s\S]*?)<\/script>/gi)].map(match => match[1]);

class TextNode {
    constructor(data) {
        this.nodeType = 3;
        this.data = data;
        this.parentElement = null;
    }
}

class Element {
    constructor(tagName = "DIV") {
        this.nodeType = 1;
        this.tagName = tagName;
        this.childNodes = [];
        this.parentElement = null;
        this.attrs = {};
        this.value = "";
        this.scrollTop = 0;
    }
    append(...nodes) {
        for (const node of nodes) {
            node.parentElement = this;
            this.childNodes.push(node);
        }
    }
    closest() { return null; }
    hasAttribute(name) { return Object.prototype.hasOwnProperty.call(this.attrs, name); }
    getAttribute(name) { return this.attrs[name]; }
    setAttribute(name, value) { this.attrs[name] = String(value); }
}

const body = new Element("BODY");
const button = new Element("BUTTON");
const buttonText = new TextNode("设置");
button.setAttribute("aria-label", "关闭");
button.setAttribute("title", "打开设置");
button.append(buttonText);
const input = new Element("INPUT");
input.setAttribute("placeholder", "搜索片名、原标题、年份、IMDb ID 或路径");
input.value = "Casino Royale";
input.scrollTop = 73;
body.append(button, input);

const privacyLink = new Element("A");
const termsLink = new Element("A");
const elements = {"#privacyLink": privacyLink, "#termsLink": termsLink};
const document = {
    body,
    documentElement: {lang: ""},
    querySelector(selector) { return elements[selector] || null; }
};
const context = {
    console,
    Object,
    String,
    Element,
    Node: {TEXT_NODE: 3, ELEMENT_NODE: 1},
    document,
    MutationObserver: class { observe() {} },
    requestAnimationFrame(callback) { callback(); },
    window: {dispatchEvent() {}},
    Event: class {}
};
vm.createContext(context);
vm.runInContext(scripts[0] + ";globalThis.i18nTest={UI_LOCALES,normalizeUILanguage,uiMessage,setUILanguage,translateUIDocument};", context);

function assert(condition, message) {
    if (!condition) throw new Error(message);
    console.log("OK  " + message);
}

context.i18nTest.setUILanguage("en-US");
assert(document.documentElement.lang === "en-US", "English updates html lang immediately");
assert(buttonText.data === "Settings", "visible text switches to English");
assert(button.getAttribute("aria-label") === "Close", "aria-label switches to English");
assert(button.getAttribute("title") === "Open Settings", "title switches to English");
assert(input.getAttribute("placeholder") === "Search title, original title, year, IMDb ID, or path", "placeholder switches to English");
assert(input.value === "Casino Royale" && input.scrollTop === 73, "language switching preserves form and scroll state");
assert(privacyLink.getAttribute("href").endsWith("/PRIVACY.en.md"), "English privacy link is selected");
assert(termsLink.getAttribute("href").endsWith("/TERMS.en.md"), "English terms link is selected");
assert(context.i18nTest.uiMessage("第 12 季 · 4") === "Season 12 · 4", "dynamic season copy is localized");
assert(context.i18nTest.uiMessage("27 项") === "27 items", "dynamic count copy is localized");

context.i18nTest.setUILanguage("zh-CN");
assert(document.documentElement.lang === "zh-CN", "Chinese restores html lang immediately");
assert(buttonText.data === "设置", "Chinese round trip restores visible source text");
assert(button.getAttribute("aria-label") === "关闭", "Chinese round trip restores aria-label");
assert(input.getAttribute("placeholder") === "搜索片名、原标题、年份、IMDb ID 或路径", "Chinese round trip restores placeholder");
assert(context.i18nTest.normalizeUILanguage("ja-JP") === "zh-CN", "unsupported Manager locale fails closed to Chinese");

const staticMarkup = html.replace(/<script[\s\S]*?<\/script>/gi, "").replace(/<style[\s\S]*?<\/style>/gi, "");
const staticChinese = [...staticMarkup.matchAll(/>([^<>]+)</g)]
    .map(match => match[1].replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&").trim())
    .filter(value => /[\u3400-\u9fff]/.test(value) && !value.includes("侯雁泽"));
context.i18nTest.setUILanguage("en-US");
assert(staticChinese.every(value => !/[\u3400-\u9fff]/.test(context.i18nTest.uiMessage(value))), "all static visible Manager copy has an English rendering");

const dynamicChinese = new Set();
for (const source of scripts.slice(1)) {
    for (const match of source.matchAll(/'(?:\\.|[^'\\\r\n])*'|"(?:\\.|[^"\\\r\n])*"/g)) {
        let value;
        try { value = vm.runInNewContext(match[0]); } catch (_) { continue; }
        if (/[\u3400-\u9fff]/.test(value)) dynamicChinese.add(value);
    }
}
assert([...dynamicChinese].every(value => {
    let visible = value.replace(/<[^>]+>/g, "").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");
    if (visible === " 项") visible = "1 项";
    return !/[\u3400-\u9fff]/.test(context.i18nTest.uiMessage(visible));
}), "all dynamic Manager UI literals have an English rendering");

const card = fs.readFileSync(path.join(windowsRoot, "engine", "technical-specs-card.js"), "utf8");
const fieldStart = card.indexOf("    const FIELD_ORDER = [");
const localeEnd = card.indexOf("    const TECH_CARD_SELECTOR", fieldStart);
const resolverStart = card.indexOf("    function resolveCardLocale()", localeEnd);
const resolverEnd = card.indexOf("    function decodeRouteValue", resolverStart);
const cardLocaleSource = card.slice(fieldStart, localeEnd) + card.slice(resolverStart, resolverEnd) +
    ";globalThis.cardI18nTest={FIELD_ORDER,CARD_LOCALES,DEFAULT_CARD_LOCALE,resolveCardLocale};";
const cardDocument = {
    documentElement: {lang: "es-ES"},
    body: {getAttribute() { return "es-ES"; }}
};
const cardContext = {Object, String, document: cardDocument};
vm.createContext(cardContext);
vm.runInContext(cardLocaleSource, cardContext);
assert(cardContext.cardI18nTest.resolveCardLocale() === "zh-CN", "unsupported Emby locale falls back to Chinese");
cardDocument.body.getAttribute = () => "en-US";
assert(cardContext.cardI18nTest.resolveCardLocale() === "zh-CN", "an explicit unsupported Emby html locale does not fall through to English");
cardDocument.documentElement.lang = "ja-JP";
assert(cardContext.cardI18nTest.resolveCardLocale() === "zh-CN", "future untranslated Emby locale also falls back to Chinese");
cardDocument.documentElement.lang = "en-GB";
assert(cardContext.cardI18nTest.resolveCardLocale() === "en-US", "English Emby variants use English card labels");
cardDocument.documentElement.lang = "zh-Hant";
assert(cardContext.cardI18nTest.resolveCardLocale() === "zh-CN", "Chinese Emby variants use Chinese card labels");
const locales = cardContext.cardI18nTest.CARD_LOCALES;
const fields = cardContext.cardI18nTest.FIELD_ORDER;
assert(fields.every(field => locales["zh-CN"].fields[field] && locales["en-US"].fields[field] === field), "card registry localizes labels without changing field keys");
assert(locales["zh-CN"].empty && locales["en-US"].empty && locales["zh-CN"].title && locales["en-US"].title, "card registry localizes aria and empty-state copy");

console.log("OK Tech Card Manager runtime localization registry and round-trip behavior");
