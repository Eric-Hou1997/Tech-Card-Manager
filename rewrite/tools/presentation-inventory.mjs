// Review candidates only. This tool never translates or edits source and does
// not treat literal coverage as proof of runtime or semantic localization.
import {readFileSync,readdirSync,writeFileSync} from 'node:fs';
import {resolve,relative,extname,dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
import {parse as parseSfc} from '@vue/compiler-sfc';
import {parse as parseTemplate,NodeTypes} from '@vue/compiler-dom';
import ts from 'typescript';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const records=[];
const han=/[\u3400-\u9fff]/u;
function files(directory){return readdirSync(directory,{withFileTypes:true}).flatMap(entry=>entry.isDirectory()?files(resolve(directory,entry.name)):[resolve(directory,entry.name)]);}
for(const path of files(resolve(root,'src')).filter(path=>['.vue','.ts'].includes(extname(path))&&!path.endsWith('/contracts.ts'))){
 const source=readFileSync(path,'utf8');
 function record(value,offset,kind,context=''){
  value=value.trim();if(!value||!(/[A-Za-z\u3400-\u9fff]/u.test(value)))return;
  if(!han.test(value)&&kind==='script'&&!/\s/.test(value))return;
  records.push({file:relative(root,path),line:source.slice(0,Math.max(0,offset)).split('\n').length,kind,context,value,review:'unclassified'});
 }
 function script(content,offset=0,expression=false){
  const prefix=expression?'const __presentation_candidate = (':'';
  const parsed=ts.createSourceFile(path,prefix+content+(expression?');':''),ts.ScriptTarget.Latest,true,ts.ScriptKind.TS);
  function walk(node){
   if(ts.isStringLiteralLike(node)||ts.isTemplateHead(node)||ts.isTemplateMiddle(node)||ts.isTemplateTail(node)){
    record(node.text,offset+node.getStart(parsed)-prefix.length,'script',ts.SyntaxKind[node.parent?.kind]??'');
   }
   ts.forEachChild(node,walk);
  }
  walk(parsed);
 }
 if(extname(path)==='.ts'){script(source);continue;}
 const {descriptor,errors}=parseSfc(source,{filename:path});if(errors.length)throw new Error(`SFC parse failed: ${relative(root,path)}`);
 for(const block of [descriptor.script,descriptor.scriptSetup])if(block)script(block.content,block.loc.start.offset);
 if(!descriptor.template)continue;
 const template=descriptor.template;const ast=parseTemplate(template.content);
 function walk(node){
  const offset=template.loc.start.offset+(node.loc?.start.offset??0);
  if(node.type===NodeTypes.TEXT)record(node.content,offset,'text');
  if(node.type===NodeTypes.INTERPOLATION)script(node.content.content,template.loc.start.offset+node.content.loc.start.offset,true);
  if(node.type===NodeTypes.ELEMENT){for(const prop of node.props){
   if(prop.type===NodeTypes.ATTRIBUTE&&prop.value&&['aria-label','title','placeholder','alt','label'].includes(prop.name))record(prop.value.content,template.loc.start.offset+prop.value.loc.start.offset,'attribute',prop.name);
   if(prop.type===NodeTypes.DIRECTIVE&&prop.exp)script(prop.exp.content,template.loc.start.offset+prop.exp.loc.start.offset,true);
  }}
  for(const child of node.children??[])walk(child);
 }
 walk(ast);
}
const unique=new Map();for(const record of records)unique.set(JSON.stringify([record.file,record.line,record.kind,record.value]),record);
const values=[...unique.values()];const report={status:'unclassified-presentation-candidates-not-localization-acceptance',files:new Set(values.map(v=>v.file)).size,occurrences:values.length,unique_messages:new Set(values.map(v=>v.value)).size,records:values};
const output=process.argv[2];if(output)writeFileSync(resolve(output),JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify({status:report.status,files:report.files,occurrences:report.occurrences,unique_messages:report.unique_messages,...(output?{output:resolve(output)}:{})}));
