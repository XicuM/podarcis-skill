// Bundles the Obsidian plugin and the VS Code extension and webview from the shared sources.
import { copyFile, mkdir } from 'node:fs/promises';
import esbuild from 'esbuild';

const common = { bundle: true, logLevel: 'info', target: 'es2021' };
await mkdir('vscode/dist', { recursive: true });
await Promise.all([
  esbuild.build({ ...common, entryPoints: ['obsidian/main.ts'], outfile: 'obsidian/main.js', format: 'cjs', platform: 'node', external: ['obsidian', 'electron'] }),
  esbuild.build({ ...common, entryPoints: ['vscode/extension.ts'], outfile: 'vscode/dist/extension.js', format: 'cjs', platform: 'node', external: ['vscode'] }),
  esbuild.build({ ...common, entryPoints: ['vscode/webview.ts'], outfile: 'vscode/dist/webview.js', format: 'iife', platform: 'browser' }),
  copyFile('vscode/webview.css', 'vscode/dist/webview.css'),
]);
