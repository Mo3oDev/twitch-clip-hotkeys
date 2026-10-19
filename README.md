# Twitch Clip Hotkeys

Windows 10/11 desktop app that creates Twitch clips of a configurable exact
duration via global hotkeys — no browser, no leaving the game.

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
