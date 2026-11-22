# Demo captures

The README needs one moving picture that shows what the project is for. This note
fixes *what* to show, in *what order*, so the recording is reproducible and does
not turn into a tour of the UI.

Assets live in `assets/`:

| file | what | state |
| --- | --- | --- |
| `assets/playground.png` | the page after one step | committed |
| `assets/playground.gif` | typing, a diagnostic, a fix, a step | recorded by Playwright |
| `assets/vscode-fix.gif` | a squiggle, the quick fix, the target picker | recorded over CDP |
| `assets/demos/hover.png` | hover on an instruction | recorded over CDP |
| `assets/demos/definition.gif` | F12 on a label | recorded over CDP |
| `assets/demos/completion.gif` | typing `ad`, the list, the accepted line | recorded over CDP |
| `assets/demos/target.gif` | command palette, then the picker | recorded over CDP |
| `assets/demos/debug.gif` | entry stop, breakpoint, one step | recorded over CDP |

## Story A: the editor catches a bad target (recommended first)

1. Open `tests/corpus/006_arch_directives.s` (a file that declares `+m`).
2. Type `mul a2, a0, a1` in `005_trap_csr.s` instead, which is `rv32i_zicsr`.
3. The squiggle appears: "`mul` needs extension `m`".
4. Click the light bulb: "enable extension m in this file" — the file gains
   `.option arch, +m` and the squiggle goes away.
5. Click the status bar item: the Quick Pick lists targets; pick `rv32i_m_zicsr`;
   the status becomes `target rv32im · set for this file`.

Why this story: it shows the one thing the project claims — the editor knows the
target, the file can declare it, and the fix is mechanical.

## Story B: debugging (second)

1. Open `002_factorial.s`, set a breakpoint on the `call factorial` line.
2. F5, **Debug this file**. The session stops at the breakpoint.
3. Step over the call; the Memory view follows `sp` and highlights changed bytes.
4. Hover a register to see its value.

## Story C: the playground (recorded automatically)

1. Type a program with `mul` while the target is `rv32i_zicsr`: the diagnostic
   appears with a one-click fix.
2. Apply the fix, then step: the registers and memory fill in.

Reproduce with:

```bash
RVH_RECORD=1 pnpm --dir web run test:e2e -- -g "record the README demo"
python3 scripts/assemble-gif.py assets/frames/playground assets/playground.gif --fps 1
```

The test is skipped unless `RVH_RECORD=1`, so a normal `test:e2e` run does not
rewrite the frames.

Two details in the recording script are deliberate: it inserts the program with
`keyboard.insertText` rather than `keyboard.type`, because typing presses Enter
between lines and the editor's auto-indent then stacks a level per line; and it
parks the caret on the line with the error, so the highlight in the still marks
the diagnostic rather than the last line.

## Recording the VS Code story

The extension can be driven without a human at the keyboard: VS Code exposes the
DevTools protocol on a port, and Playwright connects to it as if it were a browser
page.

```bash
# one terminal: a private X display and a dev host with the corpus open
Xvfb :126 -screen 0 1920x1200x24 -nolisten tcp &
DISPLAY=:126 RVH_LSP_SERVER=target/debug/rvh \
  code --no-sandbox --disable-gpu --ozone-platform=x11 \
       --extensions-dir=/tmp/rvh-e --user-data-dir=/tmp/rvh-p \
       --extensionDevelopmentPath=editors/vscode \
       --remote-debugging-port=9353 scratch/vscode-demo/ws &
# another: drive it and capture
node scripts/record-vscode.mjs      # the GIF story, into assets/frames/vscode-fix
python3 scripts/assemble-gif.py assets/frames/vscode-fix assets/vscode-fix.gif --fps 1

node scripts/capture-vscode.mjs     # per-feature frames, into assets/frames/<story>/
for story in definition completion target debug; do
  python3 scripts/assemble-gif.py assets/frames/$story assets/demos/$story.gif --fps 1
done
```

A story that is a *sequence* becomes a GIF (definition, completion, target,
debug); hover is a state, so it stays a still.

`capture-vscode.mjs` runs the stories in README order: hover, definition,
completion, target picker, debug. It needs the demo workspace to be a
copy of `tests/corpus` with `"editor.autoIndent": "none"`; the launcher in this
note writes it.

Three settings make the capture stable, and all three are about the *demo
profile*, not the product:

- `"window.zoomLevel": 2` so the text survives GitHub scaling;
- `"editor.autoIndent": "none"`, because pasting the file otherwise re-indents
  every pasted line one level deeper;
- `"chat.disableAIFeatures": true` and a hidden secondary sidebar, so the built-in
  chat does not take half the window.

`record-vscode.mjs` rewrites the buffer from `tests/corpus/005_trap_csr.s` before
each run, so a previous run leaves no extra lines behind. It also parks the mouse
in the corner between shots: a hover left open by a previous click lands in the
frame.

## How to record

Frames are cheaper than video: capture stills at the moments above, keep them in
one directory, and let the assembler build the GIF.

```bash
# 01.png, 02.png, ... in name order
python3 scripts/assemble-gif.py assets/frames/vscode-fix assets/vscode-fix.gif --fps 2
```

The assembler uses one adaptive palette for the whole set, so colours stay stable
when the GIF loops. It needs Pillow, which is only an asset-time dependency.

### Recording on a machine with a desktop

Any of these work; keep the window at 1440×900 or smaller and hide the activity
bar:

- GNOME: `Peek` or `Ctrl+Alt+Shift+R` (screen recorder, writes a `.webm`);
  `Peek` writes a GIF directly.
- OBS: record a window, export `.mp4`, convert with
  `ffmpeg -i in.mp4 -vf "fps=10,scale=960:-1" out.gif`.
- ImageMagick: `import -window root 01.png` per frame, then the assembler above.

### What a headless machine can and cannot do

Checked on this checkout:

- `xvfb-run` works and gives VS Code a display; the app starts and writes its
  profile.
- The bundled Playwright `ffmpeg` has **no** `x11grab` demuxer and no GIF
  encoder, so it cannot record a window; it can only encode PNG.
- The DevTools protocol port was not reachable during the probe, most likely
  because the sandbox gives the app a private `/dev/shm` and a private PID
  namespace. A CDP-driven capture would need `--disable-dev-shm-usage` and a
  display the app can reach, and has not been proven here.
- The playground is different: Playwright drives it headlessly and takes
  screenshots directly, so `assets/playground.gif` can be produced without a
  desktop.

Conclusion: the playground demo can be automated; the VS Code demos are recorded
by hand, from the storyboard above, on a machine with a desktop.
