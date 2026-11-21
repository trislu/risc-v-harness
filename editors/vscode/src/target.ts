import {
  commands,
  languages,
  window,
  workspace,
  type Disposable,
  type ExtensionContext,
  type LanguageStatusItem,
} from 'vscode';
import type { LanguageClient } from 'vscode-languageclient/node';

/** Targets the machine can run today. */
export const CANDIDATES = ['rv32i', 'rv32i_zicsr', 'rv32i_m', 'rv32i_m_zicsr'];

const PINS_KEY = 'rvh.pinnedTargets';

interface Effective {
  target: string;
  /** `file` when a pin decides it, `workspace` when `rvh.target` does. */
  source: 'file' | 'workspace';
}

/**
 * The language status item that answers "what ISA is this file analysed as?" and
 * lets the user change it per file.
 *
 * A workspace can mix ISAs, so the workspace setting is a default and a pin wins
 * for one document. The server learns about pins through
 * `initializationOptions.documentTargets` and `rvh/setDocumentTarget`.
 */
export class TargetStatus implements Disposable {
  private readonly item: LanguageStatusItem = languages.createLanguageStatusItem('rvh.target', {
    language: 'riscv-assembly',
  });
  private readonly disposables: Disposable[] = [];

  public constructor(
    private readonly context: ExtensionContext,
    private readonly client: () => LanguageClient | undefined,
  ) {
    this.item.name = 'RISC-V target';
    this.item.command = { title: 'Select target', command: 'rvh.selectTarget' };
    this.disposables.push(
      window.onDidChangeActiveTextEditor(() => this.update()),
      workspace.onDidChangeConfiguration((event) => {
        if (event.affectsConfiguration('rvh.target')) {
          this.update();
        }
      }),
      commands.registerCommand('rvh.selectTarget', () => this.select()),
    );
    this.update();
  }

  /** Pins by document URI, as stored in workspace state. */
  public pins(): Record<string, string> {
    return this.context.workspaceState.get<Record<string, string>>(PINS_KEY, {});
  }

  public effective(uri: string | undefined): Effective {
    const fallback =
      workspace.getConfiguration('rvh').get<string>('target') ?? 'rv32i_zicsr';
    const pinned = uri ? this.pins()[uri] : undefined;
    return pinned
      ? { target: pinned, source: 'file' }
      : { target: fallback, source: 'workspace' };
  }

  public update(): void {
    const editor = window.activeTextEditor;
    if (!editor || editor.document.languageId !== 'riscv-assembly') {
      this.item.text = '';
      this.item.detail = undefined;
      return;
    }
    const { target, source } = this.effective(editor.document.uri.toString());
    this.item.text = `target ${target}`;
    this.item.detail = source === 'file' ? 'set for this file' : 'workspace default';
  }

  private async select(): Promise<void> {
    const editor = window.activeTextEditor;
    if (!editor) {
      return;
    }
    const uri = editor.document.uri.toString();
    const current = this.effective(uri);
    const fallback = this.effective(undefined).target;

    const picks = [
      ...CANDIDATES.map((target) => ({
        label: target,
        description: target === current.target ? 'current' : '',
      })),
      {
        label: 'Use workspace default',
        description: `${fallback} (rvh.target)`,
      },
    ];
    const pick = await window.showQuickPick(picks, {
      placeHolder: 'Target ISA for this file',
      title: 'RISC-V target',
    });
    if (!pick) {
      return;
    }

    const pins = { ...this.pins() };
    const useDefault = pick.label === 'Use workspace default';
    if (useDefault) {
      delete pins[uri];
    } else {
      pins[uri] = pick.label;
    }
    await this.context.workspaceState.update(PINS_KEY, pins);

    this.client()?.sendNotification('rvh/setDocumentTarget', {
      uri,
      target: useDefault ? undefined : pick.label,
    });
    this.update();
  }

  public dispose(): void {
    this.item.dispose();
    for (const disposable of this.disposables) {
      disposable.dispose();
    }
  }
}
