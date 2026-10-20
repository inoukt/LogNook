# Subaru catalog integration assessment — 2026-10-19

Read-only inspection using two Luna xhigh agents. Source directory:
`PIDs/CatalogV2/SubaruCatalog`. Existing application changes were preserved.
No source code, database rows, adapter settings or vehicle state were changed.
No test suite or hardware request was run.

## Measured contents

| Item | Count |
| --- | ---: |
| Expanded data-monitor signal rows | 45,364 |
| Distinct underlying table rows | 20,855 |
| Repeated rows from table reuse | 24,509 |
| ECU records | 168 |
| Physical model/year entries | 96 |
| Additional pseudo-vehicle entries | 2 |
| Unbound tables / rows | 10 / 1,480 |
| Rows with conversion expressions | 41,993 |
| Rows with non-empty units | 19,560 |
| Rows with enums | 19,187 |
| Conversion-method codes | 25 |

These are research data counts, not supported/working PID counts. The apparent
12,679 route-scoped request signatures are inferred-hint groups, not confirmed
wire requests. One request can contain many signals; a row's own DID may not be
the request DID.

`catalog.json` retains more decoder detail than flattened `pids.csv` and should
be the import input. Keep ECU/vehicle mappings and aliases rather than storing
45,364 independent copies. Preserve separate protocol families: FHI-CAN,
Phase4-CAN, Phase5-CAN, CAN14230, KWP14230, SSM-KWP and OBD-15765. The source
application covers several different wire protocols.

## Import constraints

- `request_hint` is explicitly inferred from identifier width. The exports omit
  the original `CommSupport` request mappings. Example: ECU 6002, Number of DTCs,
  has hint `22 0001`, whereas the limitations document identifies the underlying
  OBD records as Mode 01. Never construct a request queue from these hints.
- The older 22,335-row analysis in `LIMITATIONS.md` is stale relative to the
  current 20,855 distinct / 45,364 expanded rows. Its hint mismatch figures
  explain a known problem but are not current measured compatibility rates.
- All ECU records have request IDs/data rates; 167 have response IDs. Missing
  addressing fields require protocol-specific interpretation. Connection/session
  setup is not exported. Do not invent addressing, setup or security sequences.
- Normal model/year entries cover 2011–2025. Five older range entries share
  vehicle ID 0; four encode 2098 despite labels indicating 98. Preserve these as
  unresolved applicability. No engine/calibration applicability is supplied.
- Numeric expressions, enums and special conversions need method-specific
  parsing and tests. Preserve raw source fields; do not assume all 25 methods
  are implemented by LogNook.
- Exclude active-test/control/configuration data: 4,568 active-test UI items,
  139 customization settings and 518 calibration-pack targets. `dids_other.csv`
  includes 2,648 active-test, 621 customization and 20 internal-info rows.

## Provenance and packaging

The documents identify a North American vendor installer and database 40.01b.
Original product metadata is retained internally. XML table filenames and generated row payloads are available;
raw XML, confirmed source URLs, upstream checksums and license terms are absent.
No redistribution grant was found in the reviewed catalog documentation.
This is a metadata finding, not a legal conclusion.

The existing app catalog is
`PIDs/LogNook_PID_Master_v0_3/data/lognook_pids.sqlite`. It is mapped into app
resources by `apps/desktop/src-tauri/tauri.conf.json`. Therefore inserting this
data into the shipped database also affects distribution; a local-only import
and a redistributable catalog build must be distinguished deliberately before
integration. Do not commit/bundle this extracted pack on the assumption that
the application's license covers it.

Audited generated input SHA-256:

- `catalog.json`: `7d0b69e88ef68e5e67eb23e8a183a7b1654fb021dd07dcbc3cc42b7a0189135d`
- `pids.csv`: `33123d404353203926425a6cc6d40ce49a4fffe179de45c7c1ce272184119820`
- `vehicle_links.json`: `7d98a82345faaf6422bbfb0fb00829af188583c204dbe65a12b48d50bc2c4a2e`

These hashes pin the local exports only; they do not supply missing upstream
provenance. Generated adapter compatibility claims in vehicle_links.json are
not physical LogNook validation evidence.

## Recommended smallest integration

1. One deterministic importer into the existing SQLite catalog, with a distinct
   research source/family and normalized definition, ECU and vehicle links.
   Hash the input; retain full available raw row JSON and source-table identity.
   Keep orphan/unresolved applicability and unsupported conversion cases visible
   as review states. Do not create a competing PID database.
2. Paginated, filtered search through the existing Rust catalog service, UI and
   typed MCP. Filter by model/year, ECU, protocol and signal name. Show source,
   decoder and request-mapping confidence. Never load the full JSON in Svelte.
3. Search-only rows initially: no Try, auto-try, polling or preset eligibility.
   Separate these from current OBD service-21/22 oem_candidates. Preserve protocol
   identity; do not flatten SSM memory addresses into ordinary DIDs. Do not use
   placeholder license values to satisfy the existing request gate.
4. Keep the user-supplied pack local until distribution metadata is resolved;
   the importer can be first-party code independent of the extracted dataset.
5. Later, recover original request mappings and test decoder/routing fidelity
   before adding any explicit research read. Physical response/decode evidence
   remains separate from imported source definitions.

Relevant app reuse points: `crates/lognook-core/src/oem_catalog.rs`,
`apps/desktop/src-tauri/src/commands/oem_catalog.rs`,
`apps/desktop/src/OemCatalogPanel.svelte`, and typed Agent Access/MCP.
The existing 704 Subaru OBDb candidate rows remain distinct from this local pack.

Before implementation, approve this bounded catalog import/search design and
resolve whether the extracted data is intended only for local use. Verification
must cover deterministic/idempotent import, deduplication, source hashes,
applicability joins, special/unknown conversion handling, no request eligibility,
and UI/MCP search parity. Run both PID database test suites and affected Rust/UI
checks after implementation. This assessment does not claim integration is done.
