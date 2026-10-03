# LogNook — research-backed engineering blueprint
Research snapshot: 2026-10-02; implementation status: 2026-10-03. Milestone 1 is implemented, and the Windows desktop app includes the searchable signal picker, recording/replay, CSV interchange and live-buffer snapshots, saved-session sharing and Save As, settings and shortcuts, threshold recording, derived signals, configurable combined/separate graphs, and smooth live graph motion between PID samples. The OBDLink SX has connected read-only to the Ford; the grouped Mode 01 path is supported by bench captures and fixtures. Long-run application stability and cross-vehicle validation remain open; see [Project Status](../PROJECT_STATUS.md).

## Current implementation

The repository contains a Rust workspace, a Tauri 2 + Svelte/TypeScript desktop app, a shared adapter contract, simulated and physical SX Mode 01 acquisition, a bounded sample pipeline, SQLite `.lognook` recording, CSV export/import, replay, automatic triggers, derived signals, and graph preferences. The simulator is explicitly labeled. Physical validation limits and the next test steps are documented in project status.

## Product promise
A Windows-first, Linux-ready automotive diagnostic/tuning *measurement* suite: connect quickly, select intelligently, record reliably, analyze deeply, share deliberately. Diagnostic logging is the initial scope, not ECU flashing, bypassing access controls or automated actuator control.

## Non-negotiable principles
1. Acquire accurately before optimizing graphs. Never substitute a displayed sampling rate for measured sample frequency.
2. One canonical `Sample`, `SignalDefinition`, `Marker`, `Source`, `Session`, `Trigger`, and `ConnectionCapability` model; no per-adapter duplication of analysis/recording/UI.
3. Modules own functionality; dependencies flow toward the common engine. No god-objects or pervasive global mutable state.
4. Offline-first, locally persisted, recoverable recordings; network features are optional.
5. Detect capabilities instead of assuming a vendor/product label guarantees them.
6. Each signal has provenance, actual unit, formula version, source ECU, quality and timing.
7. Do not access protected manufacturer-specific data unless definitions and authorization are legitimately available.
8. **Keep source-code files focused and compact**, with reusable shared functions and typed boundaries; Markdown research and documentation may be as detailed as necessary. Do not split documentation just to satisfy an arbitrary file-size target.

## Available field-test vehicles and dongles
- **2011 Ford Flex 3.5L EcoBoost:** performance-oriented generic OBD and later validated Ford-enhanced acquisition; note OBDLink SX does **not** provide Ford MS-CAN access.
- **2014 Scion xB 2.4L (2AZ-FE):** generic OBD, Toyota-specific *authorized/validated* profile testing and cross-dongle comparison.
- **Subaru, model/year/ECU dependent:** later SSM and tuning-oriented capture using verified, versioned definitions; do not assume one Subaru profile works on every Subaru.
- Hardware presently reported: **Tactrix OpenPort 2.0, OBDLink SX, generic USB ELM327**. RH-02/CANable remains an initial supported target but physical availability has not been confirmed.
- Detailed field-test matrix: `08_POC_VEHICLES.md`.

## Read in this order
- `01_PRODUCT.md`: user journeys, interface, complete feature inventory.
- `02_ARCHITECTURE.md`: technology decisions, module boundaries and repository layout.
- `03_HARDWARE_PROTOCOLS.md`: four requested interfaces, discovery and standards.
- `04_STORAGE_PERFORMANCE.md`: scheduler, timestamping, data structures and export.
- `05_VALIDATION_ROADMAP.md`: phases, hardware tests, exit criteria and open decisions.
- `06_AI_SHARING_SECURITY.md`: AI, cloud sharing, permissions and remote access.
- `07_REFERENCES.md`: primary sources and unresolved external dependencies.
- `08_POC_VEHICLES.md`: the exact available Ford/Scion field tests, a Subaru track and evidence needed to accept the POC.

Read-only acquisition comes before advanced OEM functions. Integrations must never hold the sampling thread or write to the vehicle implicitly.
