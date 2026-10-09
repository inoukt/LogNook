# LogNook

LogNook is a Windows-first, read-only OBD acquisition and recording desktop app built with Rust, Tauri 2, Svelte, and TypeScript. The current build supports simulated Mode 01 acquisition, hot-plug OBDLink SX discovery, OpenPort 2 provider diagnostics, `.lognook` recording/replay, LogNook CSV import/export (including live-buffer snapshots) and numeric FORScan `time(ms)` log import, saved-session Share/Save As actions, multi-signal graphs with smooth live motion, derived signals, and a paged read-only OEM PID research catalog. The OEM catalog includes a constrained one-shot Scion xB 2014 Mode 21 PID 01 read and manual SX-only Try once actions for eligible catalog rows; it does not add research candidates to normal polling.

Current build label: **MultiAdaptCan-v1** (`0.1.0-beta.1`).

Saved sessions support Rename, Delete (with confirmation), Open folder, Save As and Share. Share can upload a native session to [Catbox](https://catbox.moe/) for a short public link with no scheduled expiry, or save CSV locally. Anonymous uploads cannot be deleted through LogNook; service retention is not guaranteed.

**Record** starts with new samples. **Rec From Connect** includes up to 100,000 decoded samples retained in memory from the current connection, then records subsequent live samples. Its persistent marker reports evicted history. It cannot recover unselected PIDs, earlier connections, raw CAN frames, or data lost at shutdown; use CAN Capture for raw bus traffic.

## Build and run

Requires Rust, Node.js/npm, and the Windows MSVC build tools.

```powershell
cargo test --workspace
cargo check --workspace
cd apps/desktop
npm ci
npm run test:ui
npm run build
npm run tauri -- dev
```

For a packaged-protocol desktop build, run `cargo build -p lognook-desktop --features tauri/custom-protocol` from the repository root.

## Project documents

- [Current project status and next hardware validation](PROJECT_STATUS.md)
- [Engineering blueprint and architecture](LogNook_Blueprint/README.md)
- [Hardware protocol decisions](LogNook_Blueprint/03_HARDWARE_PROTOCOLS.md)
- [Validation roadmap](LogNook_Blueprint/05_VALIDATION_ROADMAP.md)
- [Future feature ideas](Addons%20for%20later.txt)
- [SX/Ford bench report](docs/hardware/2026-10-02-sx-ford.md)
- [A30 protocol and Scion xB capture research](docs/hardware/a30/README.md)
- [External-agent tasks and completed reports](Work%20to%20do%20by%20Ext%20Agent/README.md)

The repository has been verified with the simulator and captured hardware fixtures. The current grouped SX path has physical Ford bench evidence; longer application stability and cross-vehicle validation remain open. See `PROJECT_STATUS.md` before treating a feature as physically validated.
