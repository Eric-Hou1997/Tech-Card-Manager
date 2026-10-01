import {readFile} from 'node:fs/promises';
import {parse,compileScript} from '@vue/compiler-sfc';
import ts from 'typescript';
import * as vue from 'vue';
export interface Element {type:string;props:Record<string,any>;text:string;children:Element[];parent:Element|null}
export function element(type:string):Element{return {type,props:{},text:'',children:[],parent:null};}
export async function component(url:URL,dependencies:Record<string,unknown>={}){
 const source=await readFile(url,'utf8');
 const {descriptor}=parse(source,{filename:url.pathname});
 const compiled=compileScript(descriptor,{id:url.pathname,inlineTemplate:true,templateOptions:{compilerOptions:{hoistStatic:false}}});
 const code=ts.transpileModule(compiled.content,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
 const module={exports:{} as any};
 const require=(id:string)=>{if(id==='vue')return vue;if(Object.hasOwn(dependencies,id))return dependencies[id];throw Error('Unsupported component dependency: '+id);};
 new Function('require','module','exports',code)(require,module,module.exports);
 return module.exports.default;
}
export function renderer(){
 const body=element('body'),host=element('host');
 const render=vue.createRenderer<Element,Element>({
  createElement:element,createText:text=>({...element('#text'),text}),createComment:text=>({...element('#comment'),text}),
  setText:(node,text)=>{node.text=text;},setElementText:(node,text)=>{node.text=text;node.children=[];},
  patchProp:(node,key,_old,value)=>{node.props[key]=value;},
  insert:(node,parent,anchor)=>{if(node.parent)node.parent.children.splice(node.parent.children.indexOf(node),1);node.parent=parent;const index=anchor?parent.children.indexOf(anchor):-1;if(index>=0)parent.children.splice(index,0,node);else parent.children.push(node);},
  remove:node=>{if(node.parent)node.parent.children.splice(node.parent.children.indexOf(node),1);node.parent=null;},
  parentNode:node=>node.parent,nextSibling:node=>node.parent?.children[node.parent.children.indexOf(node)+1]||null,
  querySelector:selector=>selector==='body'?body:null,
 });
 return {body,host,render};
}
export function find(root:Element,id:string):Element|undefined {if(root.props.id===id)return root;for(const child of root.children){const result=find(child,id);if(result)return result;}}
