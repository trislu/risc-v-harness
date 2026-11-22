import {
  Uri,
  debug,
  type Webview,
  type WebviewView,
  type WebviewViewProvider,
} from 'vscode';

/** What the adapter reports on every stop. */
export interface MachineState {
  pc: string;
  sp: string;
  steps: number;
}

/**
 * The memory view: a hex grid that follows `sp` or `pc`.
 *
 * Bytes that changed since the previous render are highlighted, which is the one
 * thing this view has over a generic hex editor: the simulator knows exactly
 * which bytes a step touched.
 */
export class MemoryView implements WebviewViewProvider {
  public static readonly viewType = 'rvh.memory';
  private view?: WebviewView;

  public constructor(private readonly extensionUri: Uri) {}

  public resolveWebviewView(view: WebviewView): void {
    this.view = view;
    view.webview.options = {
      enableScripts: true,
      // The webview may load only from `media/`, nothing else on disk.
      localResourceRoots: [Uri.joinPath(this.extensionUri, 'media')],
    };
    view.webview.html = html(view.webview, this.extensionUri);

    view.webview.onDidReceiveMessage(async (message: { type?: string; address?: string; count?: number }) => {
      if (message?.type !== 'read' || !message.address) {
        return;
      }
      const session = debug.activeDebugSession;
      if (!session || session.type !== 'rvh') {
        this.post({ type: 'error', message: 'no active rvh session' });
        return;
      }
      try {
        const result = (await session.customRequest('readMemory', {
          memoryReference: message.address,
          count: message.count ?? 256,
        })) as { address: string; data: string };
        this.post({
          type: 'bytes',
          address: result.address,
          bytes: base64ToBytes(result.data),
        });
      } catch (error) {
        this.post({ type: 'error', message: String(error) });
      }
    });
  }

  /** Forward a new machine state to the grid. */
  public follow(state: MachineState): void {
    this.post({ type: 'state', ...state });
  }

  /** Tell the grid the session ended. */
  public clear(): void {
    this.post({ type: 'cleared' });
  }

  private post(message: unknown): void {
    void this.view?.webview.postMessage(message);
  }
}

function base64ToBytes(text: string): number[] {
  return Array.from(Buffer.from(text, 'base64'));
}

function html(webview: Webview, extensionUri: Uri): string {
  const nonce = getNonce();
  const styleUri = webview.asWebviewUri(Uri.joinPath(extensionUri, 'media', 'memory.css'));
  const scriptUri = webview.asWebviewUri(Uri.joinPath(extensionUri, 'media', 'memory.js'));
  return /* html */ `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${webview.cspSource}; script-src 'nonce-${nonce}';">
<link href="${styleUri}" rel="stylesheet">
<title>Memory</title>
</head>
<body>
<div class="bar">
  <input id="address" value="0x00000000" spellcheck="false" title="Address to read from">
  <button id="go" title="Read 256 bytes starting at the address in the box">Go</button>
  <button id="sp" title="Keep the window on the stack pointer: saved registers and locals live there">Stack (sp)</button>
  <button id="pc" title="Keep the window on the program counter: the bytes of the instruction about to run">Code (pc)</button>
</div>
<div class="hint" id="hint">manual · 256 bytes · changed bytes are highlighted</div>
<div class="grid" id="grid"></div>
<div class="status" id="status">no session</div>
<script nonce="${nonce}" src="${scriptUri}"></script>
</body>
</html>`;
}

/** A fresh nonce per render, so only the script tag above may run. */
function getNonce(): string {
  const possible = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';
  let text = '';
  for (let i = 0; i < 32; i += 1) {
    text += possible.charAt(Math.floor(Math.random() * possible.length));
  }
  return text;
}
