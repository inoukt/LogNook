# Laptop-Primary Development and Hardware Test Workflow

## Goal

Use the laptop as the authoritative LogNook build and hardware-test machine, while this desktop contributes code through GitHub. Keep the vehicle adapter physically connected to the laptop and run the app there during vehicle tests.

## Current context

The OpenPort 2 Mode 01 implementation is on GitHub `main` at `b31cd94`. The Windows app uses Node/npm, Rust, the x86 MSVC target and linker, and a separately built 32-bit J2534 helper. The laptop must have the matching adapter provider and driver installed. Toolchain availability can differ between machines; one prior Windows environment had Node but no `npm` command.

## Roles and source of truth

- The laptop keeps a normal local clone, pulls tested code from `origin/main`, builds and runs LogNook, and owns the USB/J2534 driver and physical adapter session.
- This desktop is a coding workstation. Work starts from current `origin/main`, uses a task branch named `codex/<task>`, and is pushed to GitHub. The laptop tests only after that work is merged to `main`.
- GitHub `main` is the shared release checkpoint. Each computer keeps its own working copy and generated build output; do not synchronize source folders with file-copy or cloud-sync tools.
- Record the commit SHA used for every hardware test. Do not update the laptop checkout during an active acquisition or recording.

## Laptop build and test path

The first supported hardware path is the existing read-only OpenPort 2 Mode 01 adapter. From `apps/desktop`, build the debug x86 helper with `npm run build:j2534-worker`, then run `npm run tauri dev`. The development app and helper resolve the same debug target tree. Use the simulator before connecting the vehicle to verify the desktop app and graph pipeline independently.

The laptop needs Git, Rust/Cargo, Node.js with npm, the `i686-pc-windows-msvc` Rust target, Visual Studio C++ build tools and a Windows SDK. The selected OpenPort provider remains explicitly chosen in LogNook. Do not replace drivers or fall back to an unselected provider during a test.

## Hardware evidence

For each physical session, capture the tested commit, provider name, firmware/DLL/API identity, responding ECU, supported PIDs, sample latency/rate, connection errors, and whether a recording replayed/exported. The current OpenPort build supports standard read-only Mode 01 only; Toyota/Scion service 21/22 candidates are not part of this test path.

## Out of scope

- A network USB relay, shared remote app session, or live desktop-to-laptop measurement bridge.
- Installer/release packaging of the x86 worker; Tauri bundling remains disabled.
- Changes to the OBD protocol or vehicle request scope.

## Acceptance criteria

1. The laptop can fast-forward to tested `origin/main` without overwriting local work.
2. The laptop can build the Svelte interface, Tauri development app and x86 J2534 helper from that checkout.
3. A simulator smoke test works on the laptop before hardware access.
4. The laptop can identify and connect to its selected OpenPort provider, and any actual vehicle result is recorded with the commit SHA and raw evidence needed to distinguish success from an inconclusive attempt.
5. Desktop changes reach the laptop through reviewed Git commits on `main`, with no folder-copy synchronization.
