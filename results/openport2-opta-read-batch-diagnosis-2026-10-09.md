# OpenPort 2 / OPTA read-batch diagnosis — 2026-10-09

Laptop test checkout: `C:\Users\inouk\LogNook-pid60-bitmap-fix`, branch
`codex/openport-provider-resolution-fix`, based on main `7326404`.

The owner retried LogNook with OPTA on the Ford. LogNook reported an empty
J2534 record during PID 00 discovery (ProtocolID=0, RxStatus=0, DataSize=0).
The OPTA wire log instead contains the actual ECU reply:
`00 00 07 E8 41 00 BF BE A8 93`, followed by a ReadMsgs transport timeout.
See `openport2-opta-read-batch-wire-2026-10-09.log`, a bounded text snapshot
from `%TEMP%\j2534-0404.log` before the next owner retest.

The reviewed OPTA source at revision
`97745698a8acafd055288a842c07b5b15ab0709f` waits for the requested message count.
Its transport timeout error path discards collected replies; its 04.04 FFI
error path does not update NumMsgs or fill the output messages. The installed
DLL's exact source revision is unknown, but its trace exhibits that failure.

LogNook now requests one message per ReadMsgs call, retaining the existing
bounded read window and per-ECU response collection. On ERR_TIMEOUT, a fully
untouched zero buffer is treated as no returned messages rather than decoded
as an ECU reply. Genuine partial replies and successful malformed messages
retain their existing handling. No support, decoder, or vehicle validation is
inferred from this workaround.

Verification: 24 worker tests passed; i686-pc-windows-msvc release helper build
succeeded. The owner was asked to retry once with OPTA using the existing app.
The owner confirmed that the connection works after this change. Local app and
OPTA logs confirm live Ford responses. Four selected signals (commanded
equivalence ratio, short fuel trim B1, throttle position, ignition timing) each
updated around 0.9–1 Hz with zero reported signal errors; each request spent
about 250–266 ms because the functional receive window was exhausted after an
actual reply arrived in roughly 5–30 ms. The helper is in
`target\i686-pc-windows-msvc\release\lognook-j2534-worker.exe`.

OPTA's own trace shows Close sends `atz`, explaining the observed LED cycle
after failure. No new raw commands, harness, firmware or driver changes were
introduced. No automated hardware retest was performed; LogNook MCP remains
unavailable in this turn.

## Live polling improvement

Discovery retains functional 7DF requests and the full 250 ms collection
window. Once every discovered support page has one identical standard ECU
responder, the worker retains that ECU for the session. Live requests use its
physical standard OBD route (7E8 reply maps to 7E0 request) and finish upon its
matched reply. The core still validates the ECU and response payload. Ambiguous
discovery never enables the physical route; shutdown clears it.

Verification: 26 worker tests passed, including physical-route bounds,
matched-reply completion, ambiguous discovery, and untouched timeout buffers.
The 32-bit release helper rebuilt successfully. The owner disconnected before
replacement and reconnected. RPM and throttle measured about 6.6–6.7 Hz each
with zero reported errors (56 successes per signal in one observed summary),
and request latencies approximately 13 ms and 29 ms. A subsequent four-signal
run (RPM, coolant, IAT, throttle) measured about 4.8–5.3 Hz each, with 35/35
successes per signal and zero reported errors. These selections are separate
measurements, not a controlled speed ratio.

The core adapter still added 100 ms after each completed cycle. The owner
reported a sparse/choppy throttle graph. Its sample interval is now zero:
existing bounded, serialized J2534 exchanges pace acquisition. The test
asserting 100 ms was updated; all 25 OpenPort core tests pass. The desktop
release rebuild succeeded and the rebuilt app was reopened. Physical rate
measurement without that extra delay: one selected signal, throttle position,
ran for 24.7 s. OPTA reported 822/822 samples and zero errors, about 33 Hz over
the run with a final summary rate of 35.8 Hz. This is a throttle-only result.
A later three-signal run (RPM, intake temperature, throttle) returned 39/39
samples per signal over 3.5 s at 12.2 Hz each. The newest four-signal run
(RPM, fuel trim short B2, intake temperature, throttle) returned 63/63 per
signal over 7.56 s at 9.43 Hz each, about 37.7 aggregate samples/s, with zero
reported errors. No display interpolation or invented samples were added.
