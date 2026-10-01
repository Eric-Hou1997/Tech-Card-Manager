import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';

const baseline=await readFile(new URL('../../windows/engine/technical-specs-card.js',import.meta.url),'utf8');
const current=await readFile(new URL('../web-card/technical-specs-card.js',import.meta.url),'utf8');

function replaceOnce(source:string,from:string,to:string){
 const first=source.indexOf(from);assert.notEqual(first,-1,`missing allowed card delta: ${from.slice(0,60)}`);
 assert.equal(source.indexOf(from,first+from.length),-1,`repeated allowed card delta: ${from.slice(0,60)}`);
 return source.slice(0,first)+to+source.slice(first+from.length);
}

test('the v5 Emby card retains the complete v4.1.0 renderer except reviewed version and locale adapters',()=>{
 let normalized=current;
 normalized=replaceOnce(normalized,'const WEB_CARD_VERSION = "5.0.0";','const WEB_CARD_VERSION = "4.1.0";');
 normalized=replaceOnce(normalized,`        "zh-Hant": Object.freeze({
            title: "技術規格", empty: "暫無技術規格資料",
            fields: Object.freeze({"Runtime":"正片時長","Sound mix":"聲音制式","Color":"色彩類型","Aspect ratio":"畫幅比例","Camera":"攝影器材","Laboratory":"沖印流程","Film Length":"膠片長度","Negative Format":"底片格式","Cinematographic Process":"攝影工藝","Printed Film Format":"放映格式"})
        }),
`,'');
 normalized=replaceOnce(normalized,'            if (["zh-hant", "zh-tw", "zh-hk", "zh-mo"].some(code => language === code || language.startsWith(code + "-"))) return "zh-Hant";\n','');
 normalized=replaceOnce(normalized,`        if (records.some(record => record.type === "attributes" && (record.target === document.documentElement || record.target === document.body))) {
            scheduleRender("locale-change", 0);
        } else if (records.some(mutationTouchesRenderSurface)) {
`,`        if (records.some(mutationTouchesRenderSurface)) {
`);
 normalized=replaceOnce(normalized,`        childList: true,
        attributes: true,
        attributeFilter: ["lang", "data-culture"]
`,`        childList: true
`);
 assert.equal(normalized,baseline);
});

test('the reviewed locale adapter renders Traditional Chinese without changing protocol field keys',()=>{
 const fieldStart=current.indexOf('    const FIELD_ORDER = [');
 const localeEnd=current.indexOf('    const TECH_CARD_SELECTOR',fieldStart);
 const resolverStart=current.indexOf('    function resolveCardLocale()',localeEnd);
 const resolverEnd=current.indexOf('    function decodeRouteValue',resolverStart);
 assert.ok(fieldStart>=0&&localeEnd>fieldStart&&resolverStart>localeEnd&&resolverEnd>resolverStart);
 const source=current.slice(fieldStart,localeEnd)+current.slice(resolverStart,resolverEnd)+
  ';globalThis.cardI18n={FIELD_ORDER,CARD_LOCALES,resolveCardLocale};';
 const document={documentElement:{lang:'zh-Hant'},body:{getAttribute:()=>''}};
 const context=vm.createContext({Object,String,TextEncoder,fetch:async()=>{throw Error('not used')},document});
 vm.runInContext(source,context);
 const card=(context as any).cardI18n;
 assert.equal(card.resolveCardLocale(),'zh-Hant');
 assert.equal(card.CARD_LOCALES['zh-Hant'].title,'技術規格');
 assert.ok(card.FIELD_ORDER.every((key:string)=>card.CARD_LOCALES['zh-Hant'].fields[key]));
 document.documentElement.lang='zh-HK';assert.equal(card.resolveCardLocale(),'zh-Hant');
 document.documentElement.lang='en-GB';assert.equal(card.resolveCardLocale(),'en-US');
});
