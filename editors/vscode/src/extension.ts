import { existsSync } from 'node:fs';
import type { ExtensionContext } from 'vscode';
import { DebugAdapterExecutable, Uri, debug, window, workspace } from 'vscode';
import { MemoryView, type MachineState } from './memory';
import { TargetStatus } from './target';
import {
  LanguageClient,
  TransportKind,
  type LanguageClientOptions,
  type ServerOptions,
} from 'vscode-languageclient/node';

let client: LanguageClient | undefined;

/**
 * The `rvh` executable to run.
 *
 * `RVH_LSP_SERVER` wins so a debugging session never depends on user settings;
 * `rvh.serverPath` is the user override. Otherwise the archive ships a binary
 * for its own platform, so the default is the copy inside the extension.
 */
function resolveServerPath(context: ExtensionContext): string {
  const configured =
    process.env.RVH_LSP_SERVER || workspace.getConfiguration('rvh').get<string>('serverPath');
  if (configured) {
    return configured;
  }
  const name = process.platform === 'win32' ? 'rvh.exe' : 'rvh';
  return Uri.joinPath(context.extensionUri, 'bin', `${process.platform}-${process.arch}`, name)
    .fsPath;
}

/**
 * Read the `rvh.*` settings. Called at activation and again after a change, so a
 * setting edit does not need a window reload.
 */
function readSettings(context: ExtensionContext): {
  serverPath: string;
  target: string;
  logLevel: string;
} {
  const configuration = workspace.getConfiguration('rvh');
  return {
    serverPath: resolveServerPath(context),
    target: configuration.get<string>('target') ?? 'rv32i_zicsr',
    logLevel: configuration.get<string>('logLevel') ?? 'normal',
  };
}

export function activate(context: ExtensionContext): void {
  // Never console.log from an extension: the lines land in the window's debug
  // console. The output channel is where a user looks.
  const output = window.createOutputChannel('RISC-V harness');
  const target = new TargetStatus(context, () => client);
  context.subscriptions.push(target);

  // A VSIX carries one platform's binary, so a missing file means this build
  // was installed on a platform it was not packaged for.
  const bundled = resolveServerPath(context);
  if (!existsSync(bundled)) {
    output.appendLine(`no rvh binary at ${bundled}`);
    void window.showErrorMessage(
      `rvh was not found at ${bundled}. Set rvh.serverPath, or install the ` +
        `${process.platform}-${process.arch} build of the extension.`,
    );
  }

  const startClient = (): void => {
    const settings = readSettings(context);
    const pins = target?.pins() ?? {};
    output.appendLine(`server ${settings.serverPath}, target ${settings.target}`);

    const serverOptions: ServerOptions = {
      command: settings.serverPath,
      args: ['lsp'],
      transport: TransportKind.stdio,
    };
    const clientOptions: LanguageClientOptions = {
      documentSelector: [{ scheme: 'file', language: 'riscv-assembly' }],
      initializationOptions: {
        target: settings.target,
        logLevel: settings.logLevel,
        documentTargets: pins,
      },
      synchronize: {
        fileEvents: workspace.createFileSystemWatcher('**/*.{s,S}'),
      },
    };

    client = new LanguageClient('rvh', 'RISC-V harness', serverOptions, {
      ...clientOptions,
      outputChannel: output,
    });
    void client.start().then(() => {
      const capabilities = client?.initializeResult?.capabilities;
      if (capabilities) {
        output.appendLine(`capabilities: ${Object.keys(capabilities).join(', ')}`);
      }
      // Completion is triggered by the editor, not the server. Record the two
      // settings that decide it, so a silent editor has a diagnosis.
      const editorConfiguration = workspace.getConfiguration('editor');
      output.appendLine(
        `quickSuggestions=${JSON.stringify(editorConfiguration.get('quickSuggestions'))} ` +
          `suggest.enabled=${JSON.stringify(editorConfiguration.get('suggest.enabled'))}`,
      );
    });
  };

  startClient();
  context.subscriptions.push(
    workspace.onDidChangeConfiguration(async (event) => {
      if (!event.affectsConfiguration('rvh')) {
        return;
      }
      output.appendLine('rvh settings changed; restarting the language server');
      const previous = client;
      client = undefined;
      await previous?.stop();
      startClient();
    }),
  );

  // A language-id mismatch produces silence rather than an error: the client
  // simply never asks the server anything. Say it out loud once.
  const editor = window.activeTextEditor;
  if (editor) {
    output.appendLine(`active document language: ${editor.document.languageId}`);
    if (editor.document.languageId !== 'riscv-assembly') {
      output.appendLine('that is not riscv-assembly; set files.associations for .s');
    }
  }

  context.subscriptions.push({
    dispose: () => {
      void client?.stop();
      output.dispose();
    },
  });

  // The memory view lives in the debug sidebar and follows the machine.
  const memory = new MemoryView(context.extensionUri);
  context.subscriptions.push(
    window.registerWebviewViewProvider(MemoryView.viewType, memory, {
      webviewOptions: { retainContextWhenHidden: true },
    }),
  );
  context.subscriptions.push(
    debug.onDidReceiveDebugSessionCustomEvent((event) => {
      if (event.event === 'rvh/memory') {
        memory.follow(event.body as MachineState);
      }
    }),
  );
  context.subscriptions.push(
    debug.onDidTerminateDebugSession(() => memory.clear()),
  );

  // The debug adapter is the same binary in a different mode.
  context.subscriptions.push(
    debug.registerDebugAdapterDescriptorFactory('rvh', {
      createDebugAdapterDescriptor: () =>
        new DebugAdapterExecutable(readSettings(context).serverPath, ['dap']),
    }),
  );
}

// `Thenable` is a global type in the VS Code typings, not an export.
export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}
