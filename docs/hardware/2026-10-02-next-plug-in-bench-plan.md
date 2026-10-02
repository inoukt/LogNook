# Ford bench session — 2026-10-02

**Order:** OBDLink SX coverage/rate measurements first; OpenPort2 passive capture immediately after, with the adapters tested one at a time.

## Baseline and constraints

- The user reports **at least 50 PID requests/second in OBDwiz USB mode on this same Ford**. FORScan showed COM4/FTDI at 2,000,000 baud. These are the comparison baselines; do not relabel them as LogNook measurements.
- The latest five-signal LogNook CSV contained 1,095 samples in about 65.6 seconds: approximately 16.7 PID responses/second total and 3.3 per signal.
- The `PIDs` research seed is at `C:\LogNookR\PIDs`, outside this Git checkout. The SX benchmark selects only generic Mode 01 candidates from its public, auto-requestable set, plus the five already live-verified LogNook signals, and only when the ECU advertises each PID.
- Keep ignition ON / engine OFF and vehicle requests read-only. For baud tests, use only the documented volatile `STSBR <baud>` transition and restore 115,200 before closing; never send `STWBR` or otherwise save settings to NVM. The bounded periodic-message probe uses only the five verified generic Mode 01 reads; remove each `STPPMA` handle with `STPPMD`, then issue `STPPMC` and verify normal reads before closing. Do not send reset, DTC-clear, OEM/UDS, actuator, or ECU-write requests. Close each adapter handle before switching devices or transports.

## Stage 1 — OBDLink SX

**Preflight:** disconnect the SX in LogNook and close OBDwiz/FORScan. Confirm no application owns COM4 or D2XX. Leave the SX connected to the Ford. At each rate, identify the adapter at 115,200 first. For a faster rate, issue `STSBR <baud>` at the old rate, require its `OK`, wait for the switch, set the host rate, then verify `STI`. Only after identity succeeds may the harness send Mode 01 requests. Restore with `STSBR 115200`, change the host back, and verify `STI` before closing.

**Transport matrix:** run the same harness and PID mix over:

1. VCP COM4 at 115,200 baud.
2. VCP COM4 switched to 921,600 baud (the documented ScanTool example rate).
3. VCP COM4 switched to 2,000,000 baud (the FORScan setting reported by the user).
4. D2XX at 115,200 baud with the current 1 ms latency timer.
5. D2XX at 115,200 baud with a 16 ms FTDI host latency timer as a control.
6. D2XX switched to 2,000,000 baud at the current 1 ms latency timer.
7. D2XX request for 3,000,000 baud to establish whether the adapter can generate it; the adapter rejected this rate.

The D2XX harness reads only bytes queued by FTDI and restores the previous latency timer and host baud on close. `STSBR` changes the adapter UART rate only for the current powered session; each successful test restores 115,200 and verifies `STI`. No `STWBR`, `ATPP`, or NVM write is sent. Initial host-only 921,600/1,000,000/2,000,000/3,000,000 attempts timed out at `STI`; they are retained as invalid setup probes because they changed only the PC rate. Their logs contain no Mode 01 traffic.

For each successful transport, the harness will:

1. Record `STI`, `STDI`, `STSN`, and `STPRS` responses with round-trip time.
2. Walk the Mode 01 supported-PID bitmaps from `0100` through each advertised continuation range and retain the raw responses.
3. Probe public generic Mode 01 PIDs from the seed, plus the five already verified LogNook signals. Save each raw response, expected byte width, latency, and whether it returned a matching positive response.
4. Measure sequential mixes of 1, 5, 8, 16, and 24 responding PIDs. The one-PID set is RPM; the five-PID set matches the current LogNook catalog. If fewer than 24 eligible PIDs respond, record the available count instead of padding the mix with repeated requests.
5. Report aggregate valid responses/second, per-PID samples/second, request latency p50/p95, cycle latency p50/p95, and response failures. These measurements will be compared with the user's OBDwiz baseline on the same Ford.

Prepared benchmark source: `C:\Users\inouk\AppData\Local\Temp\lognook-sx-performance\src\main.rs`. It built successfully offline. It does not change application source.

Run each case only after the previous process has exited and released its handle. Use new log files; do not overwrite prior evidence:

```powershell
$exe = 'C:\Users\inouk\AppData\Local\Temp\lognook-sx-performance\target\debug\lognook-sx-performance.exe'
& $exe vcp 115200 2>&1 | Tee-Object -FilePath 'docs\hardware\2026-10-02-sx-rate-vcp-115200.log'
& $exe vcp-switch 921600 2>&1 | Tee-Object -FilePath 'docs\hardware\2026-10-02-sx-rate-vcp-921600-switched.log'
& $exe vcp-switch 2000000 2>&1 | Tee-Object -FilePath 'docs\hardware\2026-10-02-sx-rate-vcp-2000000-switched.log'
& $exe d2xx 115200 keep 2>&1 | Tee-Object -FilePath 'docs\hardware\2026-10-02-sx-rate-d2xx-115200-queued-current.log'
& $exe d2xx 115200 16 2>&1 | Tee-Object -FilePath 'docs\hardware\2026-10-02-sx-rate-d2xx-115200-16ms.log'
& $exe d2xx-switch 2000000 keep 2>&1 | Tee-Object -FilePath 'docs\hardware\2026-10-02-sx-rate-d2xx-2000000-switched.log'
& $exe d2xx-switch 3000000 keep 2>&1 | Tee-Object -FilePath 'docs\hardware\2026-10-02-sx-rate-d2xx-3000000-switched.log'
```

## Stage 2 — OpenPort2, immediately after SX

1. Close the SX transport, disconnect and unplug the SX from the vehicle, then connect the OpenPort2 to USB and the Ford OBD port.
2. Keep the existing OPTA WinUSB binding and installed OPTA J2534 DLL; do not restore the Tactrix driver or change registrations.
3. Use the separate x86 `openport2_passive_capture.exe` probe. It verifies OPTA identity, opens CAN5 at 500 kbit/s with `SNIFF_MODE | CAN_ID_BOTH`, installs a pass filter, captures five seconds, then disconnects and closes.
4. Preserve each returned J2534 message as an opaque payload with the full `ProtocolID`, `DataSize`, `ExtraDataIndex`, `RxStatus`, `TxFlags`, timestamp, and all data bytes. Do not infer frame boundaries if OPTA combines frames. Do not fall back to ordinary CAN if the passive request fails.
5. The probe does not load `PassThruWriteMsgs`, periodic-message, programming-voltage, or ECU diagnostic request APIs. This is a passive CAN receive retry, not an OpenPort PID acquisition test.

Prepared source: [openport2_passive_capture.cpp](probes/openport2_passive_capture.cpp). Build x86 from a Visual Studio developer shell:

```bat
pushd "%TEMP%"
cl /nologo /W4 /EHsc /Fo:openport2_passive_capture.obj /Fe:openport2_passive_capture.exe "C:\Users\inouk\LogNook\docs\hardware\probes\openport2_passive_capture.cpp"
popd
```

Run it once with ignition ON / engine OFF and retain its complete host output plus the OPTA J2534 wire trace. Always verify `PassThruDisconnect` and `PassThruClose` before ending the capture.

```powershell
& "$env:TEMP\openport2_passive_capture.exe" 2>&1 | Tee-Object -FilePath 'docs\hardware\2026-10-02-openport2-passive-full.log'
```

## Completion record

The high-rate, generic-PID, and bounded periodic-message tests are complete and summarized in [the SX Ford report](2026-10-02-sx-ford.md). Stage 2 is pending: the OpenPort2 was not present in the most recent PnP inventory, so connect it before running the prepared passive capture. Do not make production transport or decoder changes until the OpenPort2 result and SX timing evidence have been reviewed.

## Additional SX test: combined SAE PID requests

The installed OBDwiz 4.36.0 release notes reveal a “Combine SAE PID Requests” (PID stuffing) option added in 4.30.0, specifically described as increasing SAE PID refresh on CAN. Five-PID stuffing is verified at about 80 PID values/s versus about 33/s with one PID per request. A six-PID request returned all six values but dropped to 63.6/s because its response needed three ISO-TP frames. Seven width-aware groups covering all 35 advertised candidates returned 350/350 values at 81.42/s. See the [SX report](2026-10-02-sx-ford.md#combined-sae-pid-probe--documented-before-transmission) and [full PID stuffing log](2026-10-02-sx-pid-stuffing.log). The probe used only standard Mode 01 reads; no OEM, DTC, actuator, or write commands.
