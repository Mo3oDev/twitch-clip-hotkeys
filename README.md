# Twitch Clip Hotkeys

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg)](https://www.rust-lang.org/)

Windows 10/11 desktop app that creates Twitch clips of a configurable exact
duration via global hotkeys — no browser, no leaving the game.

## Highlights

- **Global hotkeys, configurable duration** — bind any combo to a clip length
  (5-60s); works with the game in focus/fullscreen.
- **Exact duration when possible** — clips from the live VOD with the exact
  requested length, falling back to Twitch's classic ~30s clip endpoint when
  a VOD isn't usable (stream just started, VODs disabled, API hiccup).
- **Random clip titles** — `word-word-Ns` per clip, so they're never all
  named the same thing as the stream.
- **Auto-announces in your own chat** — posts the clip link to your channel's
  chat right after creation (best-effort; never blocks the clip itself).
- **System tray, not a taskbar window** — starts hidden, lives in the tray;
  closing the settings window hides it, it doesn't quit the hotkey listener.
- **No browser-stored secrets** — OAuth Device Code Flow (no client secret),
  tokens in Windows Credential Manager, never a plaintext file.
- **Centralized logging** — every auth/clip/hotkey event lands in
  `%APPDATA%/<id>/logs/*.log` for post-mortem debugging (release builds have
  no console). Tokens are never logged — only ids, outcomes, error kinds.

## Prerequisites

- Windows 10/11 (this app only targets `x86_64-pc-windows-msvc`; it cannot
  build or run inside WSL/Linux).
- Rust (stable, MSVC toolchain) via [rustup](https://rustup.rs).
- Node.js + npm.
- `cargo install tauri-cli --version "^2" --locked`.
- A Twitch application registered at https://dev.twitch.tv/console:
  - Type **Public** (no client secret — this app uses the OAuth Device
    Code Flow, which never stores one).
  - Any OAuth Redirect URL (e.g. `http://localhost`) — required by the
    console to save the app, unused by Device Code Flow itself.
  - Requests scopes `clips:edit` and `user:write:chat`. If you connected
    before chat-announcing existed, disconnect and reconnect once to grant
    the new scope — a cached token doesn't pick it up on its own.

## Development

```powershell
npm install
npm run tauri dev
```

The app starts with no visible window — only a system tray icon. Right-click
it for "Abrir ajustes" / "Salir".

On first run, open Settings from the tray and paste your Twitch app's
Client ID, then "Conectar con Twitch" to run the Device Code login.

## Building a release installer

```powershell
npm run tauri build
```

Produces an NSIS installer under
`src-tauri/target/release/bundle/nsis/*.exe`. Not code-signed in v1 —
Windows SmartScreen may warn on first run ("More info" → "Run anyway").

## Scope

v1 is intentionally minimal: hotkeys, configurable duration, OAuth, clip
creation, non-blocking notifications, and a tray settings UI. No local clip
history and no autostart with Windows — see the spec for the full rationale.

## License

[MIT](LICENSE)
