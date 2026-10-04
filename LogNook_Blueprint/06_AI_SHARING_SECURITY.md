# Assistant, sharing, privacy and future remote control

## Local diagnostic assistant / POC bridge
Provide a local **Diagnostic Bridge** that collects opt-in structured health snapshots: app build, OS, adapter/firmware identity, selected protocol, requested vs actual PID rates, timer histograms, timeout/retry counts, queue pressure, scrubbed packet traces, recorder health, and POC outcomes. A one-click 'Copy diagnostic report' must work offline. A future opt-in OpenAI API connection may send selected redacted data for diagnostics and function-call suggestions; do not imply this conversation has automatic access to a separate local program. Keep normal acquisition measurement-only. A dedicated local test path may issue only reviewed requests; state-changing diagnostics require explicit approval for each exact test. No unrestricted shell, raw ECU write interface, or automatic security bypass. Store API keys in OS-secured credential storage, never in logs.

## Trigger engine and alerting
A shared declarative rules engine powers auto-record, auto-marker, visual/sound alerts and optional graph annotations. Inputs: current/previous timestamped signals, time window, threshold, rate of change, boolean conditions, hysteresis, debounce and missing-data policy. A tone maps bounded engineering units to safe configurable pitch/volume; always support silence, per-device audio selection and background-session mute.

## Google Drive
Native desktop OAuth 2.0 authorization-code flow with PKCE and external system browser. Request smallest relevant `drive.file` scope; encrypt/store refresh token using OS credential APIs. Background resumable upload of finalized `.lognook`, CSV and optional redacted screenshot; provide explicit permissions (private by default), share-link handling, queued retry and cancellation. Never upload VIN, license-plate or customer notes without explicit user selection.

## Datazap
Datazap's public site confirms CSV-oriented automotive log visualization, not a publicly documented external upload API in research reviewed on 2026-10-02. Do not confuse it with the unrelated Datazapp marketing-data API. MVP share button: produce Datazap-compatible wide CSV, open Datazap site and offer copy/file handoff. Genuine one-click authenticated upload needs their confirmed supported API or explicit partner arrangement; do not automate private login endpoints or promise credentials can be embedded safely.

## Remote LAN server (deferred)
The backend may expose a strictly opt-in local web server: bind to loopback by default; require explicit 'Enable LAN', paired clients, rotating tokens, and TLS or an encrypted tunnel. Remote sessions remain READ-ONLY; remote clients cannot issue vehicle diagnostic or state-changing commands. Remote recording requires local approval, session permissions, action audit, idle timeout, and emergency revoke. No default WAN exposure, unrestricted HTTP API, or unauthenticated network binding. Prefer a dashboard state/delta protocol over literal screen mirroring.

## Security and data governance
Treat CSV fields, formulas, DBC/ODX content, vehicle-provided strings and imported files as untrusted. Strict parser bounds, no formula code execution, safe HTML escaping, bounded archive extraction, signed release artifacts and vulnerability dependency reviews. VIN is sensitive contextual metadata: local unless the user deliberately exports/shares. Raw traces can include vehicle identifiers and customer data. Crash reports and usage telemetry are opt-in. The UI must clearly separate measurement acquisition (read-only), reviewed read-test requests, and the dedicated per-test approval flow for state-changing diagnostics. Never expose unrestricted raw injection; flashing/programming remains outside the product scope.
