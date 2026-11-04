export type Mcp = string | Record<string, unknown>;
export interface Config {
  agents: string[];
  repos: Record<string, string | null>;
  mcp: Mcp[];
  skills: Record<string, unknown>[];
}
export interface Server { description: string; setup?: string }
export interface Repo { state: 'missing' | 'local' | 'repo'; url: string | null; empty?: boolean }
export interface State {
  config: Config;
  catalogue: Record<string, Server>;
  repos: Record<string, Repo>;
  actions?: Record<string, string>;
}

export const REPOS = ['sources', 'wiki', 'workspace'];
export const TARGETS = ['claude', 'codex', 'copilot', 'cursor', 'opencode', 'pi', 'vscode'];

/** Apply the form's choices; custom MCP servers and extra skills from podarcis.yaml are kept. */
export function nextConfig(
  config: Config, repos: Record<string, string>, agents: string[], recommended: string[],
): Config {
  return {
    ...config,
    agents,
    repos: Object.fromEntries(REPOS.map((r) => [r, repos[r]?.trim() || null])),
    mcp: [...recommended, ...config.mcp.filter((m) => typeof m !== 'string')],
  };
}

/** Render the setup form as plain DOM, shared by the Obsidian view and the VS Code webview. */
export function renderForm(
  el: HTMLElement, state: State, message: string,
  actions: { apply: (config: Config) => void; reload: () => void },
): void {
  const doc = el.ownerDocument;
  const add = <K extends keyof HTMLElementTagNameMap>(
    parent: HTMLElement, tag: K, text?: string, cls?: string,
  ): HTMLElementTagNameMap[K] => {
    const node = doc.createElement(tag);
    if (text) node.textContent = text;
    if (cls) node.className = cls;
    parent.appendChild(node);
    return node;
  };
  const check = (parent: HTMLElement, label: string, checked: boolean) => {
    const row = add(parent, 'label', undefined, 'podarcis-check');
    const box = add(row, 'input');
    box.type = 'checkbox';
    box.checked = checked;
    add(row, 'span', label);
    return box;
  };
  el.replaceChildren();
  el.classList.add('podarcis-panel');
  if (message) add(el, 'pre', message, 'podarcis-message');

  add(el, 'h4', 'Repositories');
  const urls: Record<string, HTMLInputElement> = {};
  for (const name of REPOS) {
    const repo = state.repos[name];
    const row = add(el, 'div', undefined, 'podarcis-repo');
    add(row, 'span', `${name}/`, 'podarcis-name');
    const detail = repo.state === 'repo' ? `clone of ${repo.url ?? 'no remote'}` : repo.state === 'local' ? 'local folder' : 'not created';
    add(row, 'span', state.actions?.[name] ? `${detail} · ${state.actions[name]}` : detail, 'podarcis-muted');
    urls[name] = add(row, 'input');
    urls[name].type = 'text';
    urls[name].placeholder = 'Git URL, or empty for a local folder';
    urls[name].value = state.config.repos[name] ?? '';
  }

  add(el, 'h4', 'Agents');
  const targets = [...new Set([...TARGETS, ...state.config.agents])];
  const agents = targets.map((t) => [t, check(el, t, state.config.agents.includes(t))] as const);

  add(el, 'h4', 'Recommended MCP servers');
  const servers = Object.entries(state.catalogue).map(([name, server]) => {
    const box = check(el, name, state.config.mcp.includes(name));
    add(el, 'div', server.description, 'podarcis-muted');
    if (server.setup) add(el, 'div', server.setup, 'podarcis-muted podarcis-setup');
    return [name, box] as const;
  });
  const custom = state.config.mcp.filter((m) => typeof m !== 'string').map((m) => String(m.name));
  add(el, 'div', custom.length ? `Custom servers in podarcis.yaml: ${custom.join(', ')}` : 'Add custom servers and skills in podarcis.yaml.', 'podarcis-muted');

  const buttons = add(el, 'div', undefined, 'podarcis-buttons');
  const apply = add(buttons, 'button', 'Apply', 'mod-cta');
  const reload = add(buttons, 'button', 'Reload');
  apply.onclick = () => {
    apply.disabled = reload.disabled = true;
    apply.textContent = 'Applying…';
    actions.apply(nextConfig(
      state.config,
      Object.fromEntries(REPOS.map((r) => [r, urls[r].value])),
      agents.filter(([, b]) => b.checked).map(([t]) => t),
      servers.filter(([, b]) => b.checked).map(([n]) => n),
    ));
  };
  reload.onclick = actions.reload;
}
