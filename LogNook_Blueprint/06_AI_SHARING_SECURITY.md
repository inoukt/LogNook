# Assistant, sharing, privacy and future remote control

## Local diagnostic assistant / POC bridge
Provide a local **Diagnostic Bridge** that collects opt-in structured health snapshots: app build, OS, adapter+firmware identity, selected protocol, requested vs actual PID rates, timer histograms, timeout/retry counts, queue pressure, packet traces scrubbed by default, recorder health and POC test outcomes. A one-click 'Copy diagnostic report' must work fully offline. A future opt-in OpenAI API connection can send selected redacted data for conversational diagnostics and function-call suggestions; do not imply this ChatGPT conversation has automatic live access to a separate local program. Maintain an explicit approval step for configuration changes. No unrestricted shell, ECU write operations or automatic security bypass. API keys reside in OS-secured credential storage, never in logs.

## Trigger engine and alerting
A shared declarative rules engine powers auto-record, auto-marker, visual/sound alerts and optional graph annotations. Inputs: current/previous timestamped signals, time window, threshold, rate of change, boolean conditions, hysteresis, debounce and missing-data policy. A tone maps bounded engineering units to safe configurable pitch/volume; always support silence, per-device audio selection and background-session mute.

## Google Drive
Native desktop OAuth 2.0 authorization-code flow with PKCE and external system browser. Request smallest relevant `drive.file` scope; encrypt/store refresh token using OS credential APIs. Background resumable upload of finalized `.lognook`, CSV and optional redacted screenshot; provide explicit permissions (private by default), share-link handling, queued retry and cancellation. Never upload VIN, license-plate or customer notes without explicit user selection.

## Datazap
Datazap's public site confirms CSV-oriented automotive log visualization, not a publicly documented external upload API in research reviewed on 2026-10-02. Do not confuse it with the unrelated Datazapp marketing-data API. MVP share button: produce Datazap-compatible wide CSV, open Datazap site and offer copy/file handoff. Genuine one-click authenticated upload needs their confirmed supported API or explicit partner arrangement; do not automate private login endpoints or promise credentials can be embedded safely.

## Remote LAN server (deferred)
The backend may expose a strictly opt-in local web server: start bound to loopback; explicit 'Enable LAN' setting with paired clients, rotating tokens and TLS or encrypted tunnel. Default remote sessions READ-ONLY; remote recording commands require physical local approval, session permissions, complete action audit, idle timeout and emergency revoke. No default WAN exposure, generic unrestricted HTTP API or unauthenticated network binding. Prefer sending a dashboard state/delta protocol over literal screen mirroring so bandwidth and input behavior remain predictable.

## Security and data governance
Treat CSV fields, formulas, DBC/ODX content, vehicle-provided strings and imported files as untrusted. Strict parser bounds, no formula code execution, safe HTML escaping, bounded archive extraction, signed release artifacts and vulnerability dependency reviews. VIN is sensitive contextual metadata: local unless the user deliberately exports/shares. Raw traces can include vehicle identifiers and customer data. Crash reports and usage telemetry are opt-in. The UI must clearly separate acquisition (read-only), raw injection (explicitly permitted) and any later ECU-changing workflow (different product safety process).
