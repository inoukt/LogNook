# LogNook first vehicle/ECU POC plan

## Bench and vehicle protocol

Hardware: OBDLink SX, generic USB ELM327 and Tactrix OpenPort 2.0. Native CANable/RH-02 remains an integration target, not a verified device. Record firmware, driver version, VIN only with permission, ECU IDs, ECU CALID/CVN where available, ISO protocol, vehicle battery voltage and test temperature.

Normal acquisition and background polling are read-only. Standing project authorization covers finite, source-backed read probes on a connected test vehicle; keep them paced, record negative responses, and stop if communication degrades. Do not sweep undocumented identifiers or infer compatibility from brand alone. DTC clearing, resets, erase, coding/writes, session/security access, routines, and actuator tests require explicit approval for the exact test and are never automatic. Flashing/programming and security bypass are prohibited. Follow root `AGENTS.md`.

1. Discover USB interfaces, identify each adapter, time initial connection. Log serial settings/J2534 channel and actual ECU protocol.
2. On key on/engine off, perform generic support bitmap discovery and carefully query VIN/CALID where supported.
3. Save a per-ECU support map for standard PIDs; cross-check responses at idle, light throttle and after warmup in a stationary safe testing environment.
4. Measure achieved RPM/MAP/ECT/MAF polling frequency alone, then as an 8/16/24-PID mix. Log ECU error rates, timeout distribution, stale samples and source timestamps separately.
5. Disconnect adapter and reconnect; verify that existing recording inserts a gap event and no synthetic/interpolated measured samples are silently created.
6. Export CSV and reload in LogNook; confirm units and multi-ECU identity. Repeat on two hardware interfaces and compare median and tail request latency.

### 2011 Ford Flex 3.5 EcoBoost

Initial standard subset: RPM (010C), MAP (010B), baro (0133), ECT (0105), IAT (010F), MAF (0110), throttle (0111), trims (0106–0109), voltage (0142), commanded lambda (0144) where *advertised*. Gauge boost is a **derived channel** from absolute MAP minus barometric pressure, with timestamp and sensor-location caveats. Do not falsely name MAP as "manifold boost commanded", and do not assume generic 0170/0172 are available.

Enhanced Ford research: boost desired/actual, turbine or wastegate commands, charge-air temps, high- and low-pressure fuel target/actual, VCT target/actual, knock, per-cylinder counters, transmission slip, torque management and network module discovery. The bundled Ford Mustang reference DIDs are **not Flex definitions**. Investigate Ford Flex year-specific OBDb and proprietary licensed information separately; adapters may not access every Ford network.

### 2014 Scion xB 2.4L 2AZ-FE

Initial standard subset: RPM, ECT, IAT, MAF, throttle, trims, MAP (only if supported), spark advance and module voltage. For enhanced reads, prefer exact-model Scion-xB source candidates, including Mode 21/22 definitions where applicable; test only their exact documented request/addressing and keep them unverified until live ECU evidence supports the decoder. Do not auto-apply generic Toyota definitions.

Enhanced Toyota research: VVT actual/target, learned fuel trim zones, idle control, per-cylinder misfire history, knock correction, cooling fan duty, transmission fluid temperature and body/ABS ECU signals. An OEM DID absent in an identified ECU must not become a hard communication error loop.

### Subaru (varying models and years)

Record VIN, model year, engine family and **exact ECU fingerprint** before selecting a RomRaider-derived SSM group or OEM DID set. Subaru `000E` in the example base definition is an SSM memory offset for engine speed, not the J1979 RPM PID. Confirm command format and exact supported ECU inheritance before reading any SSM memory location.

Baseline tuning channels (where verified): RPM, MAF, MAP, baro, throttle, IAT, ECT, ignition timing, knock correction (FBKC/FLKC as applicable), DAM/IAM, boost and WGDC, VVT angles, AFR/target lambda and fuel trims. Preserve references to ECU versions and original units because negative learned knock values and data scaling are ECU/version-specific.

### Operational gates

- General public SAE parameter reference: can show in searchable catalog; automatic request **only** after support bitmap/matching protocol indicates availability.
- Enhanced DID or SSM offset: show as a research candidate until exact ECU, request header, protocol, response parsing and decoding are validated. Source-backed read probes follow the standing read authorization; any session/security step or state-changing action uses the root per-test gate.
- Imported DBC passive CAN channel: listener only; explicit transmit controls remain out of initial scope.
- Every test result records evidence, expected raw response, computed physical value, unit, timestamp and verification confidence.
