# wa-bot — pure-Rust WhatsApp anti-delete bot

Standalone Rust binary. No Node.js runtime, no napi-rs bridge, no fallback to the
JavaScript implementation.

## What it does

- Connects to WhatsApp Web via [`whatsapp-rust`](https://crates.io/crates/whatsapp-rust)
  with a SQLite store (`SqliteStore`), so the session survives restarts.
- Logs in with a pairing code when `PAIR_PHONE` is set, otherwise prints a QR code.
- Caches incoming messages in a bounded in-memory map (500 entries, 7-day TTL,
  FIFO eviction when full).
- Intercepts `ProtocolMessage` `REVOKE` events, ignores revokes by the bot
  itself, looks the original up in the cache, and forwards a copy with a
  "message deleted" header to `TARGET_JID`.
- Serves `GET /health` on `0.0.0.0:$PORT` for Render Web Services.
- `!antidelete on|off` toggles the feature at runtime (default: on).

## Build

```sh
cargo build --release
```

Windows needs the MSVC toolchain (`rustup default stable-msvc` plus VS Build
Tools with the C++ workload); SQLite is compiled from source via the `bundled`
feature, so no system `sqlite3.lib` is required.

## Run

```sh
PORT=8080 \
PAIR_PHONE=6281234567890 \
TARGET_JID=6281234567890@s.whatsapp.net \
DB_PATH=/data/whatsapp.db \
RUST_LOG=info \
./target/release/wa-bot
```

| Variable | Meaning |
|---|---|
| `PAIR_PHONE` | Phone number for pairing-code login (digits, international). Omit to use a QR code. |
| `TARGET_JID` | Chat that receives anti-delete notifications. If unset, notifications go to this bot's own account ("message yourself"). Never defaults to a hardcoded number. |
| `PORT` | HTTP health-check port (default `8080`). |
| `DB_PATH` | SQLite file holding the WhatsApp session (default `whatsapp.db`). |
| `RUST_LOG` | `tracing` filter, e.g. `info` or `debug`. |

## Deploy on Render

`render.yaml` in the repository root defines a Docker Web Service built from
`Dockerfile.rust`, with a 1 GB disk mounted at `/data` so the WhatsApp session
persists. Set `PAIR_PHONE` and `TARGET_JID` as secret/synced env vars.

## Scope

This binary currently implements the anti-delete core and the `!antidelete`
command. The other JavaScript commands (downloaders, stickers, OCR, Jarvis AI)
are **not** ported yet; the Node.js bot in the repository root is unchanged
apart from the static command registry needed to unblock the migration.
