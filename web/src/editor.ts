// A CodeMirror 6 editor with no RISC-V grammar of its own.
//
// Highlighting comes from the engine's own CST later; for now this is a plain
// editor so the panels can be built and measured first.

import { basicSetup } from 'codemirror';
import { EditorState, StateEffect, StateField } from '@codemirror/state';
import { Decoration, EditorView, type DecorationSet, type ViewUpdate } from '@codemirror/view';

/// The line the machine is about to execute.
const setExecutingLine = StateEffect.define<number>();

const executingLine = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, transaction) {
    let next = value.map(transaction.changes);
    for (const effect of transaction.effects) {
      if (!effect.is(setExecutingLine)) {
        continue;
      }
      if (effect.value < 1) {
        next = Decoration.none;
      } else {
        const number = Math.min(effect.value, transaction.state.doc.lines);
        const line = transaction.state.doc.line(number);
        next = Decoration.set([Decoration.line({ class: 'cm-executing' }).range(line.from)]);
      }
    }
    return next;
  },
  provide: (field) => EditorView.decorations.from(field),
});

export interface Editor {
  view: EditorView;
  text(): string;
  setText(text: string): void;
  focus(): void;
  /// Highlight the line the machine will run next. Leaves the cursor alone.
  markLine(line: number): void;
  /// Highlight that line and move the cursor there, scrolling if needed.
  focusLine(line: number): void;
}

export function createEditor(
  parent: HTMLElement,
  initial: string,
  onChange: (text: string) => void,
): Editor {
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc: initial,
      extensions: [
        basicSetup,
        executingLine,
        EditorView.updateListener.of((update: ViewUpdate) => {
          if (update.docChanged) {
            onChange(update.state.doc.toString());
          }
        }),
        EditorView.theme({
          '&': { height: '100%', fontSize: '14px' },
          '.cm-content': { fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace' },
          '.cm-scroller': { overflow: 'auto' },
          '.cm-executing': {
            backgroundColor: 'rgba(74, 144, 217, 0.16)',
            boxShadow: 'inset 2px 0 0 #4a90d9',
          },
        }),
      ],
    }),
  });

  return {
    view,
    text: () => view.state.doc.toString(),
    setText: (text: string) => {
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text } });
    },
    focus: () => view.focus(),
    markLine: (lineNumber: number) => {
      view.dispatch({ effects: setExecutingLine.of(lineNumber) });
    },
    focusLine: (lineNumber: number) => {
      const number = Math.min(Math.max(lineNumber, 1), view.state.doc.lines);
      const line = view.state.doc.line(number);
      view.dispatch({
        effects: setExecutingLine.of(lineNumber),
        selection: { anchor: line.from },
        scrollIntoView: true,
      });
    },
  };
}
