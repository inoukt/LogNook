# Codex Task — Integrate LogNook PID Database v0.2

Use `data/lognook_pids.sqlite` as the authoritative development catalog.

## Requirements

1. Create a small Rust data-access layer for:
   - curated signal search
   - source-corpus search
   - OEM candidate search
   - brand/model filtering
2. Never auto-poll a manufacturer-specific candidate with `auto_request = 0`.
3. Keep vehicle identification and definition selection separate from graph/display code.
4. Select the most specific compatible definition:
   - ECU/software verified
   - model/year verified
   - model candidate
   - make fallback
   - standard OBD fallback
5. Show research/validation state in developer/advanced UI.
6. Cache supported/verified signals per vehicle fingerprint without changing the source definition.
7. Preserve raw bytes and decoder/version provenance in recordings.
8. Add a development command that prints:
   - chosen source corpus
   - request header
   - service + DID
   - decoder formula
   - expected unit
   - validation state
9. Add tests proving an unverified OEM row cannot be added to an automatic preset.
10. Do not copy the database into multiple application modules. Use one catalog service/API.

## First target

Implement catalog queries and a Ford research browser. It should search both the three existing
curated Ford rows and any locally synced Ford candidates. This catalog task must not add state-changing commands to measurement definitions, presets, or polling. Physical validation uses root `AGENTS.md`: source-backed reads use the standing authorization; state-changing tests require approval for the exact test. Flashing/programming remains prohibited.
