#!/usr/bin/env bash
#
# Drive the real `rvh lsp` binary over stdio, the way an editor would.
#
# This covers the parts a unit test cannot: Content-Length framing, process
# lifecycle, and the exact bytes on the wire.

set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cargo build --quiet --manifest-path "$root/Cargo.toml" -p rvh-cli
rvh="$root/target/debug/rvh"

send() { printf 'Content-Length: %d\r\n\r\n%s' "${#1}" "$1"; }

out=$({
    send '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}'
    send '{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///a.s","text":"adii a0, a1, 1\n"}}}'
    send '{"jsonrpc":"2.0","id":2,"method":"textDocument/diagnostic","params":{"textDocument":{"uri":"file:///a.s"}}}'
    send '{"jsonrpc":"2.0","id":3,"method":"textDocument/hover","params":{"textDocument":{"uri":"file:///a.s"},"position":{"line":0,"character":1}}}'
    send '{"jsonrpc":"2.0","id":4,"method":"textDocument/codeAction","params":{"textDocument":{"uri":"file:///a.s"},"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":4}},"context":{"diagnostics":[]}}}'
    send '{"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":"file:///a.s"},"contentChanges":[{"text":"addi a0, a0, 1\n"}]}}'
    send '{"jsonrpc":"2.0","id":5,"method":"textDocument/diagnostic","params":{"textDocument":{"uri":"file:///a.s"},"previousResultId":"1"}}'
    send '{"jsonrpc":"2.0","id":6,"method":"textDocument/hover","params":{"textDocument":{"uri":"file:///a.s"},"position":{"line":0,"character":1}}}'
    send '{"jsonrpc":"2.0","id":7,"method":"textDocument/semanticTokens/full","params":{"textDocument":{"uri":"file:///a.s"}}}'
    send '{"jsonrpc":"2.0","id":8,"method":"textDocument/completion","params":{"textDocument":{"uri":"file:///a.s"},"position":{"line":0,"character":0}}}'
    send '{"jsonrpc":"2.0","method":"exit"}'
} | "$rvh" lsp)

check() {
    case "$out" in
    *"$1"*) ;;
    *)
        echo "lsp-smoke: missing \`$1\`" >&2
        exit 1
        ;;
    esac
}

check '"hoverProvider":true'
check '"method":"window/logMessage"'
check '"diagnosticProvider"'
check '"kind":"full"'
check 'sem.unknown-instruction'
check 'did you mean `addi`'
check '"newText":"addi"'
check '"items":[]'
check 'format `I`'
check '"data":[0,0,4,0,0'
check '"label":"addi"'
check '"range":{"start":{"line":0'

# Editors append the transport flag; a server that rejects it looks like a crash.
out=$({
    send '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}'
    send '{"jsonrpc":"2.0","method":"exit"}'
} | "$rvh" lsp --stdio)
check '"hoverProvider":true'

echo "lsp-smoke: OK"
