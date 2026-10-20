# Subaru OEM catalog integration — 2026-10-19

Implemented in `C:\Users\inouk\LogNook-can-event-freeze-frame`, preserving the
existing working changes. Luna xhigh agents implemented the importer and shared
catalog service/UI/MCP, with a separate Luna xhigh review. No hardware commands,
driver changes, firmware changes, commit or push were performed.

## Delivered

- Local import into the authoritative PID SQLite database, with a separately
  versioned extension. The base catalog remains schema version 3; extension
  version is 1. Existing OEM candidates remain unchanged at 37,093 rows.
- 20,855 normalized definitions and 45,364 ECU-linked entries. Reused source
  tables share decoder storage; links retain their ECU/alias provenance.
- 98 vehicle records, 168 ECU records, 4,086 vehicle–ECU links and 10 orphan-table
  notices. There are 29,516 per-ECU inferred request-hint groups; these are not
  verified unique wire commands or compatibility counts.
- OEM → Subaru OEM catalog search by name/DID, model/year, ECU and protocol.
  Results use 50-row pages; full decoder/source details load only on demand.
- Typed MCP `search_subaru_catalog` and `get_subaru_catalog_detail`, backed by the
  same read-only Rust service.
- A representable entry can prefill one exact CAN Workbench Mode 21/22 identifier,
  route and address format. The scan configuration opens for review. Selection
  transmits nothing and does not require an ordinary OBD connection. Existing
  explicit start/stop/capture controls own the test.

36,829 entries are currently eligible for CAN Workbench preparation, all in the
FHI-CAN family. Other protocols, Phase 4/5 addressing, unsupported extra ECU
address bytes and incomplete routes remain searchable with reasons. 41,971 rows
have supported decoder field shapes; that does not establish decoded live
measurements. An unknown decoder does not block a representable raw-response test.

All imported rows retain `auto_request=0`, `vehicle_validation=not_tested` and
inferred request state. This change does not make them ordinary continuous
graph/log channels. Those require confirmed request mapping, transport and
decoder support; no graph values are fabricated from source definitions.

## Data preservation and deployment

The original source folder was not overwritten. Original product metadata is
retained internally; user-facing naming is Subaru OEM catalog. Public source
origin is kept distinct from physical vehicle validation. Source licensing
metadata is incomplete and not fabricated. The source dataset remains local.
Default database build scripts do not depend on or automatically import it.

The authoritative database is now 147,873,792 bytes. Its original 57,049,088-byte
version was backed up byte-for-byte; location and checksum are in
[the import report](subaru-catalog-import-2026-10-19.md). This enriched local
artifact is not a public redistribution artifact and was not committed/pushed.

The existing `target/release/pids/lognook_pids.sqlite` and
`target/debug/pids/lognook_pids.sqlite` deployment copies were refreshed from the
authoritative database. All three SHA-256 hashes match:
`9014c411462644819cd0c567e942b08347fbfa21756f566eab02004c509591f2`.

## Review and verification

Review fixes cover independent model/year matching, valid year ranges, nullable
request hints, structured vehicle-scope formatting, UI/backend detail shape,
separate prefill availability from OBD connection state, preserving CAN address
format, and excluding unsupported Phase 4/5 addressing from preparation. Query
counts avoid generating the full vehicle-scope JSON for every text-search row.

- Database v0.2/v0.3 checks and focused importer fixture tests passed after import.
  SQLite integrity is `ok`; foreign-key violations: zero.
- Six focused core tests passed, including actual-catalog search/details. The
  local-pack smoke is subsequently marked ignored by default, so normal public
  fixture tests do not require the owner-supplied pack.
- 236 UI tests passed; Svelte check reported zero errors/warnings and production
  UI build passed. Evidence: [UI tests](subaru-catalog-ui-tests-2026-10-19.txt),
  [UI build](subaru-catalog-ui-build-2026-10-19.txt),
  [core tests](subaru-catalog-focused-2026-10-19.txt).
- `cargo test --workspace --locked -j 4`: **731 passed**, zero failed across 32
  targets; one local-pack smoke ignored by default. [Workspace log](subaru-catalog-workspace-tests-2026-10-19.txt).
- The optional actual-catalog smoke passed when explicitly run with
  `cargo test -p lognook-core authoritative_catalog_search_and_detail_smoke --lib --locked -- --ignored --nocapture`.
  [Local smoke log](subaru-catalog-local-smoke-2026-10-19.txt).
- `cargo fmt --all -- --check` and `git diff --check`: passed.

The first workspace run found the old MCP tool-list expectation omitted the two
new catalog tools. The assertion and new input-schema checks were updated; the
fresh full run above passed.

Physical vehicle responses, decoder correctness on a real ECU and native desktop
visual acceptance were not tested. No ordinary polling capability is inferred
from an imported row or successful catalog lookup.

Usage: [Subaru catalog guide](../docs/SUBARU_CATALOG.md).

## Local release artifact

`cargo build -p lognook-desktop --release --features tauri/custom-protocol --locked -j 4`
passed. [Release log](subaru-catalog-release-build-2026-10-19.txt).

Executable:
`C:\Users\inouk\LogNook-can-event-freeze-frame\target\release\lognook-desktop.exe`
(32,738,304 bytes; modified 2026-10-19 15:14:41 UTC).
The UI is embedded; a development server is not required. Bundling remains
disabled, so no installer was produced. The existing matching OpenPort helper
was not changed by this catalog feature. The app was left closed.
