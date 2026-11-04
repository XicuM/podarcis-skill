import { App, FileSystemAdapter, ItemView, Plugin, PluginSettingTab, Setting, WorkspaceLeaf } from 'obsidian';
import { Config, renderForm, State } from '../shared/form';
import { runVault } from '../shared/run';

const VIEW = 'podarcis-setup';
const DEFAULTS = { uv: 'uv', skill: '.agents/skills/podarcis' };

export default class PodarcisPlugin extends Plugin {
  settings = { ...DEFAULTS };

  async onload() {
    this.settings = { ...DEFAULTS, ...(await this.loadData()) };
    this.registerView(VIEW, (leaf) => new SetupView(leaf, this));
    this.addRibbonIcon('library', 'Podarcis setup', () => this.open());
    this.addCommand({ id: 'open-setup', name: 'Open setup panel', callback: () => this.open() });
    this.addSettingTab(new SettingsTab(this.app, this));
  }

  async open() {
    const leaf = this.app.workspace.getLeavesOfType(VIEW)[0] ?? this.app.workspace.getRightLeaf(false);
    if (!leaf) return;
    await leaf.setViewState({ type: VIEW, active: true });
    await this.app.workspace.revealLeaf(leaf);
  }

  run(config?: Config) {
    const root = (this.app.vault.adapter as FileSystemAdapter).getBasePath();
    return runVault(this.settings.uv, this.settings.skill, root, config);
  }
}

class SetupView extends ItemView {
  private state?: State;

  constructor(leaf: WorkspaceLeaf, private plugin: PodarcisPlugin) {
    super(leaf);
  }

  getViewType() { return VIEW; }
  getDisplayText() { return 'Podarcis'; }
  getIcon() { return 'library'; }

  async onOpen() {
    await this.refresh();
  }

  async refresh(config?: Config) {
    if (!this.state) this.contentEl.setText('Loading Podarcis configuration…');
    let message: string;
    try {
      const { state, log } = await this.plugin.run(config);
      this.state = state;
      message = log;
    } catch (error) {
      message = (error as Error).message;
    }
    if (!this.state) {
      this.contentEl.empty();
      this.contentEl.createEl('pre', { text: message, cls: 'podarcis-message' });
      this.contentEl.createEl('p', { text: `Check the uv command and the skill path (${this.plugin.settings.skill}) in this plugin's settings. Before the first setup, point the skill path at a Podarcis checkout.` });
      this.contentEl.createEl('button', { text: 'Retry' }).onclick = () => this.refresh();
      return;
    }
    renderForm(this.contentEl, this.state, message, {
      apply: (next) => this.refresh(next),
      reload: () => this.refresh(),
    });
  }
}

class SettingsTab extends PluginSettingTab {
  constructor(app: App, private plugin: PodarcisPlugin) {
    super(app, plugin);
  }

  display() {
    this.containerEl.empty();
    const field = (name: string, desc: string, key: keyof typeof DEFAULTS) =>
      new Setting(this.containerEl).setName(name).setDesc(desc).addText((text) =>
        text.setPlaceholder(DEFAULTS[key]).setValue(this.plugin.settings[key]).onChange(async (value) => {
          this.plugin.settings[key] = value.trim() || DEFAULTS[key];
          await this.plugin.saveData(this.plugin.settings);
        }));
    field('uv command', 'Name or absolute path of uv; apps started from the desktop may not see your shell PATH.', 'uv');
    field('Podarcis skill', 'Skill folder, relative to the vault or absolute. Setup installs it at the default path.', 'skill');
  }
}
