import { spawn } from 'node:child_process';
import * as path from 'node:path';
import type { Config, State } from './form';

/** Run the vault's Podarcis script and parse its JSON; stderr becomes the log or the error. */
export function runVault(
  uv: string, skill: string, root: string, config?: Config,
): Promise<{ state: State; log: string }> {
  const script = path.resolve(root, skill, 'scripts', 'vault.py');
  const args = config ? ['setup', '--stdin'] : ['config'];
  return new Promise((resolve, reject) => {
    const child = spawn(uv, ['run', '--quiet', script, '--root', root, ...args], { cwd: root });
    let out = '';
    let err = '';
    child.stdout.on('data', (d) => (out += d));
    child.stderr.on('data', (d) => (err += d));
    child.on('error', (e) => reject(new Error(`Could not start ${uv}: ${e.message}`)));
    child.on('close', (code) => {
      if (code === 0) resolve({ state: JSON.parse(out), log: err.trim() });
      else reject(new Error(err.trim() || `vault.py exited with status ${code}`));
    });
    child.stdin.end(config ? JSON.stringify(config) : '');
  });
}
