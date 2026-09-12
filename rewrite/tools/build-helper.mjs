import {execFileSync} from 'node:child_process';
import {mkdirSync,copyFileSync} from 'node:fs';
const explicit=process.argv[2];
if(explicit || process.env.TAURI_ENV_PLATFORM==='linux') {
 const arch=explicit?.replace('-unknown-linux-gnu','') ?? process.env.TAURI_ENV_ARCH;
 if(!['x86_64','aarch64'].includes(arch))throw new Error(`Unsupported Linux architecture ${arch}`);
 const triple=`${arch}-unknown-linux-gnu`;
 const profile=process.env.TAURI_ENV_DEBUG==='true'?'debug':'release';
 execFileSync('cargo',['build','--locked','--manifest-path','src-tauri/Cargo.toml','-p','tcm-core','--bin','tcm-maintenance-helper','--target',triple,...(profile==='release'?['--release']:[])],{stdio:'inherit'});
 mkdirSync('src-tauri/binaries',{recursive:true});
 copyFileSync(`src-tauri/target/${triple}/${profile}/tcm-maintenance-helper`,`src-tauri/binaries/tcm-maintenance-helper-${triple}`);
}
