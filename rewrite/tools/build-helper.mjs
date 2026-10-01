import { execFileSync } from 'node:child_process';
import { mkdirSync, copyFileSync } from 'node:fs';
import { resolve } from 'node:path';
const arguments_ = process.argv.slice(2);
const explicit = arguments_.find(value => !value.startsWith('--'));
const debug = arguments_.includes('--debug') || process.env.TAURI_ENV_DEBUG === 'true';
const platform = process.env.TAURI_ENV_PLATFORM === 'darwin' ? 'macos' : process.env.TAURI_ENV_PLATFORM;
const host = process.platform === 'darwin' ? 'macos' : process.platform === 'win32' ? 'windows' : process.platform;
if (explicit || ['linux', 'windows', 'macos'].includes(platform ?? host)) {
  const targetPlatform = platform ?? host;
  const arch = process.env.TAURI_ENV_ARCH ?? (process.arch === 'arm64' ? 'aarch64' : 'x86_64');
  const suffixByPlatform = {windows: 'pc-windows-msvc', linux: 'unknown-linux-gnu', macos: 'apple-darwin'};
  const triple = explicit ?? `${arch}-${suffixByPlatform[targetPlatform]}`;
  if (!/^(x86_64|aarch64)-(unknown-linux-gnu|pc-windows-msvc|apple-darwin)$/.test(triple)) {
    throw new Error(`Unsupported maintenance-helper target ${triple}`);
  }
  const profile = debug ? 'debug' : 'release';
  const suffix = triple.endsWith('windows-msvc') ? '.exe' : '';
  execFileSync('cargo', ['build', '--locked', '--manifest-path', 'src-tauri/Cargo.toml', '-p', 'tcm-core', '--bin', 'tcm-maintenance-helper', '--target', triple, ...(profile === 'release' ? ['--release'] : [])], { stdio: 'inherit' });
  mkdirSync('src-tauri/binaries', { recursive: true });
  const targetDirectory = process.env.CARGO_TARGET_DIR ? resolve(process.env.CARGO_TARGET_DIR) : resolve('src-tauri/target');
  copyFileSync(`${targetDirectory}/${triple}/${profile}/tcm-maintenance-helper${suffix}`, `src-tauri/binaries/tcm-maintenance-helper-${triple}${suffix}`);
}
