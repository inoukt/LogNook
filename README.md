# LogNook

LogNook is a Windows-first desktop application for vehicle measurement, recording and CAN analysis, built with Rust, Tauri 2, Svelte and TypeScript. The current app includes simulated and hardware OBD paths, including read-only generic Mode 01 logging through Candlelight GS-USB on 11-bit/500 kbit/s CAN, `.lognook` recording/replay, CSV and FORScan log interchange, multi-signal live graphs, derived signals, a paged OEM research catalog, local Vehicle/ECU profiles with explicit signal-selection Resume previews, offline VIN decoding and provenance-aware diagnostic history, and a CAN Workbench for traffic capture, event analysis, field decoding, scans, manual sends and one-pass replay of a selected ID from a live or bounded saved capture. Generic OBD acquisition and supported diagnostic reads are read-oriented; CAN Workbench transmission is explicit, local and evidence-recorded. Adapter and vehicle support varies, so check [current project status](PROJECT_STATUS.md) and the relevant hardware report before relying on a result.

Current build label: **MultiAdaptCan-v1** (`0.1.0-beta.1`).

Saved sessions support Rename, Delete (with confirmation), Open folder, Save As and Share. Share can upload a native session to [Catbox](https://catbox.moe/) for a short public link with no scheduled expiry, or save CSV locally. Anonymous uploads cannot be deleted through LogNook; service retention is not guaranteed.

**Record** starts with new samples. **Rec From Connect** includes up to 100,000 decoded samples retained in memory from the current connection, then records subsequent live samples. Its persistent marker reports evicted history. It cannot recover unselected PIDs, earlier connections, raw CAN frames, or data lost at shutdown; use CAN Capture for raw bus traffic.

This checkout currently has no repository-wide `LICENSE` file. GitHub's [licensing guidance](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/licensing-a-repository) explains that a public repository without a license remains subject to default copyright rules. The owner wants free redistribution and modification while preventing paid resale; that policy is source-available, not legally open source under the standard definition. See the [licensing policy proposal](docs/licensing-policy-proposal.md). The OEM PID database has separate source and licensing requirements documented in [its project guide](PIDs/LogNook_PID_Master_v0_3/docs/03_SOURCES_LICENSING.md).

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
- [Contributor setup and development workflow](CONTRIBUTING.md)
- [Documentation map and maintenance guidance](docs/README.md)
- [Agent Access and MCP setup, tools, and feature-parity rules](docs/AGENT_ACCESS.md)
- [Engineering blueprint and architecture](LogNook_Blueprint/README.md)
- [Hardware protocol decisions](LogNook_Blueprint/03_HARDWARE_PROTOCOLS.md)
- [Validation roadmap](LogNook_Blueprint/05_VALIDATION_ROADMAP.md)
- [Future feature ideas](Addons%20for%20later.txt)
- [SX/Ford bench report](docs/hardware/2026-10-02-sx-ford.md)
- [A30 protocol and Scion xB capture research](docs/hardware/a30/README.md)
- [External-agent tasks and completed reports](Work%20to%20do%20by%20Ext%20Agent/README.md)

The repository has been verified with the simulator and captured hardware fixtures. The current grouped SX path has physical Ford bench evidence; longer application stability and cross-vehicle validation remain open. See `PROJECT_STATUS.md` before treating a feature as physically validated.
