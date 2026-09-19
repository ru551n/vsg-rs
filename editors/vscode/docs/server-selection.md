# Choosing a server

The extension runs an executable and talks to it. Which one is the only thing these settings
decide.

| `vsg-rs.server.mode` | Runs |
|---|---|
| `systemPath` (default) | `vsg-rs` from `PATH` |
| `userPath` | the executable named by `vsg-rs.server.path` |

## A local build

For working on vsg-rs itself:

```jsonc
"vsg-rs.server.mode": "userPath",
"vsg-rs.server.path": "/home/you/git/vsg-rs/target/debug/vsg-rs"
```

Changing either setting restarts the server, so a rebuild needs only **vsg-rs: Restart Server**
from the command palette — not a reload of the window.

## When it cannot start

The extension says so, and the reason is in the **vsg-rs** output channel
(**vsg-rs: Show Output**). The usual causes are that `vsg-rs` is not installed, or that
`server.path` points at something that is not there.

## Versions

The extension and the server are versioned together: an extension numbered `0.11.x` expects the
`vsg-rs` of the same minor version. `vsg-rs --version` prints what you have.
