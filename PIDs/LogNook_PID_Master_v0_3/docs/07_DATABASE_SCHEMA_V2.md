# Database Schema v2

## Layer 1 — `signals`

Bundled definitions already present in the LogNook research package. These include standards-based
signals and curated research candidates.

Important columns include:

- `signal_id`
- `layer`
- `brand`
- model/year/ECU filters
- service / identifier / memory offset
- request/response headers
- bit layout and formula
- unit
- source information
- research and validation state
- `auto_request`

## Layer 2 — `source_corpora`

An index of remote definition sets.

It records:

- OEM family, badge and model
- make-fallback vs model-specific scope
- GitHub repository and config path
- exact Git blob SHA
- command/signal counts
- diagnostic services represented
- license and source URL
- import state

A source being indexed does **not** mean its signal rows are bundled.

## Layer 3 — `oem_candidates`

Definitions synced from an upstream source.

Every imported row starts inert:

- `auto_request = 0`
- `vehicle_validation = not_tested`
- `research_state = community_reference`
- `ecu_filter = REQUIRES_INDIVIDUAL_ECU_VERIFICATION`

The table preserves source payload JSON so a decoder can be audited without going back to the
upstream repository.

## Definition selection

LogNook should resolve signal implementations by specificity and evidence rather than by name alone.

Suggested score inputs:

1. exact ECU software/calibration match
2. exact model + year + powertrain
3. exact model + generation
4. OEM family fallback
5. protocol compatibility
6. physical validation state
7. source confidence

A lower-specificity definition must not silently override a higher-specificity definition.

## Canonical signal concepts

The UI should eventually expose concepts such as:

- Engine RPM
- Boost / manifold pressure
- Low-side fuel pressure
- High-side fuel pressure
- Lambda / AFR
- Knock correction
- Wastegate command
- Intake/exhaust cam angle
- Transmission temperature
- Wheel speed
- Battery voltage

A concept can map to many OEM implementations. Do not store a single universal DID on the concept.

## Polling safety

Only validated, read-only measurement definitions promoted through the validation workflow may become auto-requestable. The standing read-test authorization permits source-backed physical probes without promoting them or enabling polling. State-changing commands are not signal definitions and require exact per-test user approval in a separate test path; they must never run from a preset or background acquisition. Flashing/programming is prohibited.

Recommended validation states:

- `not_tested`
- `responds_unverified_decode`
- `decode_plausible`
- `vehicle_verified`
- `ecu_variant_verified`

Automatic presets should require the appropriate verified state for manufacturer-specific data.

## Subaru catalog schema extension 1

The Subaru source uses separate tables because its source rows include proprietary memory/protocol families and inferred request hints that do not fit a verified Mode 21/22 candidate. Importing it does not create rows in `oem_candidates`.

- `subaru_source_releases` stores the neutral display name, source revision, local export SHA-256, licensing state, source URL when known, and original package metadata JSON. The local hash pins this export file; it is not an upstream Git blob SHA.
- `subaru_vehicle_specs` and `subaru_ecu_definitions` preserve source vehicle/ECU metadata. Vehicle display labels omit source-tool suffixes; raw labels stay in the internal metadata JSON. Invalid year ranges are retained as raw JSON with `year_range_state='unresolved'` and no interpreted start/end year.
- `subaru_vehicle_ecu_links` preserves source applicability and per-vehicle addressing metadata.
- `subaru_signal_definitions` stores one canonical raw monitor row per `(source_table, source_row_ordinal)`: 20,855 rows in the current export.
- `subaru_signal_ecu_links` preserves all 45,364 ECU-scoped rows, including aliases, table provenance, and source-row context without repeating the full decoder JSON.
- `subaru_request_hints` keeps the service/identifier hint separate from decoded channels. Every mapping is `inferred`; the source export does not retain the underlying communication-support record needed to treat it as a verified request.
- `subaru_orphan_tables` records the source table names/counts that have no vehicle link; it does not invent missing decoder contents.
- `subaru_catalog_search` joins definitions to ECU, request-hint, source, and vehicle-scope metadata. Use `LIMIT`/`OFFSET` for paging and filter vehicle applicability through `EXISTS` on `subaru_vehicle_ecu_links` and `subaru_vehicle_specs` to avoid multiplying results.

All rows return `auto_request=0`, `vehicle_validation='not_tested'`, and `research_state='review_required'`. Manual testing is controlled by the application gate and explicit operator action; the database import alone never authorizes a request. The source license is not cleared, no source URL is recorded, and these data are not approved for redistribution.
