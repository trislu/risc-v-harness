# ADR-0020: a shared playground state lives in the URL fragment

- Status: accepted
- Date: 2026-11-22
- Related: [ADR-0016](0016-playground-is-a-demo-surface.md)

## Context

The playground is deployed as static files on GitHub Pages. Readers compare small
programs and post them in issues and chat; telling someone to "paste this and set
the target to rv32im" loses the target half the time.

The state to share is exactly two things today: the source text and the target.
The content locale is already pinned by `?lang=`, so a link can carry the whole
view.

## Decision

**The shared state travels in the fragment, not the query or a server.**

- The fragment is `#t=<target>&s=<base64url-of-UTF-8-source>`. The source is
  encoded as UTF-8 bytes and then base64url, so a Chinese comment survives and the
  value needs no percent-escaping.
- `t` is absent-tolerant: without it the page uses `rv32i_zicsr`, and an unknown
  target falls back to the default instead of failing to load.
- Opening a link replaces the page's state: the editor is seeded and the target
  rebuilt before the first assemble.
- The Share button writes the current state into the fragment with
  `history.replaceState` and copies the whole URL. It does not push history
  entries, and it does not update the fragment on every keystroke: a typed program
  is not a link until someone asks for one.
- A fragment is never sent to the server, which matters for `rvh` because the
  deployed host is a static one and the source may be a snippet someone does not
  want in an access log.

**Compatibility**: readers ignore parameters they do not know, so adding a key is
backward compatible. Renaming or changing the encoding of `s` is not; it would
need a new key (`s2`) and one release of reading both.

## Consequences

- A link is self-contained and works offline once the page is cached; nothing is
  stored by the host.
- Very long programs make long URLs; browsers and chat clients accept a few
  kilobytes, and a program that large belongs in a file anyway.
- The encoding is part of the page's public surface: the browser suite covers it
  (`a share link restores the program and the target`) so a refactor cannot drop
  it silently.

## Alternatives

- **Query parameters (`?s=`)**. Rejected: longer, escaped differently in every
  chat client, and sent to the host. `lang` stays a query parameter because it
  predates this and is harmless to share.
- **A short-link service.** Rejected: a static deployment with no backend, and a
  dependency that can disappear.
- **Update the fragment on every edit.** Rejected: it would fill the history and
  leak every keystroke into the address bar.
