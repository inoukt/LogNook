# Engineering specification — signal registry

## 1. Terminology: PID vs DID vs memory address vs CAN signal

- SAE J1979 Service 01 uses one-byte **PIDs** (e.g. `01 0C` for RPM), and service 09 uses its own one-byte identifiers (`09 02` for VIN).
- Service 02 is a *freeze-frame service*, not merely another current-value PID library. It uses a frame/snapshot selector and has exceptions.
- OEM Service `21` requests use one-byte local identifiers; Service `22 XXXX` requests use *data identifiers* (**DIDs**). Their meaning can differ by ECU, brand, software version, or model year. An ECU may require different addressing or an authorized session even for a read. Treat each exact source-backed request as a candidate; do not guess or infer support.
- Subaru SSM requests use Subaru-specific transport and **memory offsets**. A memory location such as `000E` must never be interpreted as DID `22 000E` or SAE PID `01 0E`.
- Passive CAN signals are `(network, extended_flag, CAN_ID, byte bit start, bit length, endianness, signedness, scale, offset, multiplexor)`, preferably backed by DBC/ARXML provenance, not diagnostic services.
- SAE J1939 is another distinct identity space: PGNs/SPNs, source addresses and per-network qualifiers.
- ASAM ODX/PDX and A2L are future OEM/engineering definition imports, not universal PID lists.

## 2. Unique identity

A proper uniqueness key must include family, source pack revision, ECU identity, addressing, request, response selector, and decoded channel. One DID may contain multiple sub-channels at different `bit_offset` positions. Avoid using the display name as the key.

A minimal model is `SignalDefinition`, `RequestDefinition`, `DecodedChannel`, `ApplicabilityPredicate`, `SourceRecord`, `ValidationRecord`, `DriverCapability`, `VehicleECUIdentity`.

The v0.1 flat CSV is an interchange seed, not the final fully normalized production schema. Refactor to separate request identity and channel identity when importing full multi-channel OEM packs.

## 3. Research state vs actual availability

**Two independent axes must never be merged:**

- Research evidence: `public_reference`, `community_reference`, `review_required`, `licensed_official_crosscheck` (only when appropriate), `rejected`.
- Vehicle confirmation: `not_tested`, `identified ECU`, `requested`, `responded`, `byte_width_verified`, `formula_verified`, `repeatability_verified`, `approved`, `rejected_for_vehicle`.

Even published, correct manufacturer-specific definitions can be wrong for a different ECU. Only standard discovered generic PIDs (and separately approved enhanced DIDs) may enter the default polling queue.

## 4. Safe generic discovery

On connection: identify physical interface and ISO protocol; identify responding ECU(s); query `01 00` and walk the supported-PID bitmaps **only while the next support bitmap is advertised** (0x20/0x40 etc); selectively query `09 02` VIN after user privacy policy; do not assume `0102` is a valid current-value reading. Offer missing PIDs as unavailable rather than hammering the ECU.

Multiple ECUs may answer a functional OBD request. Keep responding ECU identity and never silently coalesce measurements from different physical sources into one time series. Preserve raw responses and UTC+monotonic timestamps.

Default universal PIDs can be offered on Ford, Scion/Toyota and Subaru, **subject to actual support bitmap and adapter capability**. Some standard turbo PIDs and oil temperature PIDs are optional and may be absent even on applicable engines.

## 5. Enhanced discovery

For a physical read test, require a reviewed source configuration and record ECU identity, model/year/engine/software applicability, addressing, and adapter support. Standing project authorization covers finite source-backed read-only probes; a required session/security-access step is gated separately under root `AGENTS.md`. The legacy `obdb_import.py` extracts only Service 22 records; the v0.3 `obdb_parse.py` deep-import path accepts read-only Services 21 and 22. Both preserve source fields and keep candidates disabled for polling. Mapping source `dbgfilter` to production compatibility is prohibited without review. Never sweep undocumented identifiers. Keep state-changing requests out of this measurement catalog and automatic polling.

The seed's sample OEM byte offsets/formulas are **review candidates**. Full source configurations may include `bix`, `blsb`, signedness, `eax`, response format details, composite signals, `nullmin/nullmax`, mappings and year overrides. A full decoder must implement these precisely. Never treat extracted `raw` scaling alone as sufficient verification.

## 6. Decode correctness

Validate expected response service (`41`, `49`, `61`, `62`, or SSM), source ECU, PID/DID, transport reassembly, minimum payload length, byte indexing after echoed identifiers, bit endian convention, sign extension, unavailable sentinels, and scaling. Expose confidence and state through the UI.

The `formula` field is **not arbitrary executable code**. Compile an allowlisted expression AST or a typed decoder DSL; reject network access, function calls outside approved operations and side effects. Derived user formulas belong to a different isolated computation layer from immutable source decoders. Typed decoder schemas are preferable to ad-hoc expression eval.

For multi-value standard PIDs, subchannels include their own presence masks and invalid sentinels. EGT temperature probes and dual temperatures must not be displayed unless their support bit is active. Engine-load percentage, injector pulsewidth and fuel trim definitions require ECU-specific validation if taken from OEM packs.

## 7. Runtime performance

Group compatible requests by ECU/transport and actual measured response characteristics. Decode once, fan out measurements to recording, trigger engine, graph model and export. Cache verified capability by ECU hardware/software fingerprint with expiration/recheck; never infer compatibility solely from VIN/brand.

Precompute typed decoder bytecode or Rust implementations at pack load, outside the acquisition hot path. Maintain a safety rate limit for unknown/slow ECUs. Preserve raw messages in trace mode for repeatable formula verification.

## 8. Storage and versioning

Version packs independently of executable releases. Pin source URL, commit SHA/tag, upstream license, import timestamp, exact filter and user verification. Never overwrite a confirmed user decoder with a newer upstream candidate without a review and rollback path.

For read-only packaged data use SQLite with FTS5 where available. For production logs store separately (native `.lognook` session format) rather than inserting fast-sampling data into the catalog database.

Reported vehicle command maps belong in `vehicle_supported_commands`, separate from OEM `21`/`22` candidates. Preserve command codes and ECU headers as supplied; do not infer command namespace, decoder validity, year applicability, or automatic polling permission from a report.
