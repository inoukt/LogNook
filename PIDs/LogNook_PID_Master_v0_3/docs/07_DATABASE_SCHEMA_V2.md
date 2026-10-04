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
