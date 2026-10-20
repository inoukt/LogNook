# Subaru OEM catalog

The owner-supplied measurement catalog is imported into the same SQLite catalog
that serves existing OEM definitions. It has a separate, versioned Subaru
extension and preserves its original protocol families. Source origin, request
mapping confidence, decoder fields and physical vehicle validation are distinct.

## Find and select an entry

1. Open **OEM**, then **Subaru OEM catalog**.
2. Search by signal name, Signal ID, DID, ECU or protocol. Unnamed entries use
   their Signal ID as the display name, falling back to the source PID ID if
   needed. This name is shared by the catalog, MCP and Live logging. Use the
   model/year and ECU filters to narrow applicability. Entries without a source
   name sort after named entries across all pages of the current filtered
   results. Source/decoder details load
   only when expanded. Cards show a compact vehicle preview; `+ N more` expands
   the full applicability list. **Details** opens decoder and source metadata.
3. Inspect the ECU route and request hint. An inferred hint is a research lead;
   the row's own DID is not necessarily the request identifier for that signal.
4. For an FHI-CAN entry with a supported decoder, choose **Add to Live**. Connect an OBD
   adapter, switch to **Live**, and select its **Log** checkbox. LogNook then
   reads the selected Mode 21/22 request through the ordinary serialized
   acquisition owner; the Log checkbox also adds the signal to Graph. Clear the
   checkbox to stop the selected Subaru plan. Signal rows show
   `SUBARU · UNVERIFIED`; samples retain raw response bytes and catalog SHA
   provenance and use suspect quality until physical ECU validation is recorded.
5. For a route the CAN Workbench can represent, **CAN Workbench** still
   fills one exact Mode 21/22 identifier and request/response IDs. Selection
   sends nothing. Review the configuration and use the explicit CAN Workbench
   start/stop and capture workflow for a raw-response test.
6. For an `SSM-KWP` row, select **OpenPort 2 → Subaru SSM K-Line** and connect.
   Add the row to Live signals, then select **Log**. The serialized
   OpenPort owner uses the J2534 ISO9141 channel at 4,800 baud, initializes the
   selected ECU once per connection with a fixed SSM2 initialization exchange,
   and reads selected 24-bit source addresses using bounded SSM2 address reads.
   It batches addresses by ECU and publishes decoded samples to the shared
   graph/recording pipeline. It does not send the inferred `22 xxxx` hint as a
   wire service. The full source request/response headers and raw SSM2 response
   frames remain preserved for review. If a conversion method is unknown or its
   enum labels are missing, LogNook still graphs the source byte/bit field as a
   raw `code` value when the layout is present; it keeps `decoder_supported`
   false so the number is not presented as a decoded engineering value.

An unsupported transport, extended ECU address, incomplete route or missing
source byte/bit layout remains searchable with its reason shown. For SSM-KWP,
unknown conversion methods and missing enum labels use a raw `code` graph when a
source layout is present. CAN Workbench raw-response tests do not establish
decoded numeric channels for ordinary graphs or logging.

FHI-CAN rows use exact 11-bit Mode 21/22 routes with supported big-endian
scalar/enum decoders. SSM-KWP rows use their source memory address, 4,800-baud
profile and byte/bit layout through the dedicated OpenPort K-Line mode. Unknown
SSM conversion methods and unlabeled enums are graphable as raw `code` values;
missing routes, addresses or layouts remain searchable and unscheduled. All
Subaru rows remain `auto_request = 0`,
`not_tested`, and outside automatic presets. Starting a graph read is an explicit
operator selection; samples remain suspect until physical ECU validation is
recorded. The implementation maps the first request-header byte to the ECU
address and the response-header byte to tester address F0; the original header
strings are preserved. The `$20` byte in the Engine request header is retained
as source metadata and does not enter SSM2 packet framing.

## Local import

From `PIDs/LogNook_PID_Master_v0_3`:

```powershell
python tools/import_subaru.py --db data/lognook_pids.sqlite --source-dir ../CatalogV2/SubaruCatalog
```

Use the machine's documented Python executable if `python` is not on PATH.
Back up the database before an import. The importer preserves the base catalog
and adds normalized source, definition, ECU, request-hint and applicability data.
Repeated imports of the same input do not duplicate definitions. Generated input
hashes and internal source metadata preserve traceability. Active-test,
customization, security and programming data are excluded from the measurement
catalog.

The application may resolve a runtime resource copy at
`target/release/pids/lognook_pids.sqlite` or `target/debug/pids/lognook_pids.sqlite`.
Refresh those copies from the authoritative database after local import. They are
deployment copies of the same catalog, not separate sources of PID definitions.

This is owner-supplied local data. Original source licensing information is
incomplete; source origin does not supply a redistribution grant. The first-party
importer is separate from the extracted dataset. Do not include the local data
or an enriched runtime database in a public release without resolving its source
and distribution metadata. The importer does not fabricate source URLs or license
values to pass the existing OBD request gate.

## Agent Access / MCP

`search_subaru_catalog` uses the same Rust query service as UI search. It supports
query, model, year, ECU, protocol, offset and bounded page size.
`get_subaru_catalog_detail` loads the decoder and source-row provenance for one
result ID. These tools do not require a vehicle connection and do not transmit.

MCP can inspect returned route/hint/eligibility fields. For SSM-KWP, connect with
the `open_port2_subaru_ssm` adapter choice, then use `oem_logging.start_subaru`
with candidate IDs from `search_subaru_catalog` and the current adapter ID and
connection generation. `stop_subaru` stops only that plan. UI and MCP use the
same serialized owner and polling path; samples retain catalog SHA provenance,
raw returned memory bytes and suspect quality. Catalog lookup never transmits,
starts a scan or changes acquisition.

## Verification scope

Importer fixture tests cover normalization, source identity, aliases, unknown
applicability and read-only/inert state. Existing v0.2/v0.3 database checks remain
applicable. Rust tests cover query filters, null fields, request preparation and
typed results. UI tests cover source display, selection and non-transmitting
handoff. Software fixtures verify SSM2 frame construction, checksum/route parsing,
K-Line request grouping and publication through the common sample pipeline.
Physical compatibility and rate still need a run on the actual Subaru ECU and
OpenPort 2 adapter.

## Protocol references

The SSM2 frame layout, 4,800-baud ISO9141 profile, `$BF` ECU initialization
packet and `$A8` address-read packet follow the archived
[LibSSM2 protocol notes](https://github.com/src0x/LibSSM2/blob/master/SSM2_Protocol.txt)
and the read-address behavior in the
[FreeSSM implementation](https://github.com/Comer352L/FreeSSM/blob/master/src/SSMprotocol.cpp).
The notes identify themselves as an older community protocol reference and
apply to SSM2; they are not a Subaru or Tactrix compatibility certification.
LogNook preserves raw J2534 response bytes and keeps imported ECU mappings
unverified until physically checked on the exact adapter and vehicle.
