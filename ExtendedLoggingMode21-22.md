# Enhanced OBD Mode 21/22 Logging
## Architecture, Discovery, Caching, Optimization, and Agent Implementation Reference

## 1. Purpose

This document defines how the application should discover, validate, cache, and log manufacturer-specific diagnostic data using diagnostic services commonly referred to as:

- Service/Mode `0x21`
- Service/Mode `0x22`

The application's existing standard OBD-II initialization, adapter setup, vehicle protocol detection, Mode 01 PID detection, and basic ECU discovery are assumed to already work.

This subsystem is specifically responsible for enhanced/manufacturer-specific logging.

The design goal is:

> Connect to an unknown vehicle, identify useful enhanced logging requests as quickly as practical, begin logging immediately, learn additional supported requests over time, cache everything learned, and reuse the cache on future connections.

The system must prioritize logging speed over exhaustive diagnostics.

This is a logging application, not a coding/programming/flashing application.

---

# 2. Primary Goals

The enhanced logging subsystem should:

1. Support Mode/Service `0x21`.
2. Support Mode/Service `0x22`.
3. Discover usable Mode 21 identifiers automatically.
4. Discover Mode 22 identifiers intelligently.
5. Avoid exhaustive 16-bit Mode 22 scanning during normal startup.
6. Detect positive and negative diagnostic responses correctly.
7. Track results per ECU.
8. Preserve raw response bytes even when no decoder exists.
9. Match known requests against an OEM/enhanced PID database.
10. Cache discoveries against a vehicle/ECU fingerprint.
11. Validate cached requests quickly on reconnect.
12. Benchmark request latency.
13. Calculate request timeouts dynamically.
14. Optimize polling frequency.
15. Support requests that return multiple signals.
16. Support multi-DID Mode 22 requests when the ECU allows them.
17. Continue optional discovery in spare logging bandwidth.
18. Avoid entering diagnostic sessions or security access unless explicitly supported by a known profile.
19. Never allow discovery activity to significantly interrupt high-priority logging.

---

# 3. Non-Goals

The automatic discovery system does NOT need to:

- flash ECUs
- perform coding
- change adaptations
- write DIDs
- perform actuator tests
- clear DTCs
- perform security unlocking automatically
- brute-force security access
- change ECU programming sessions automatically
- understand the meaning of arbitrary unknown bytes
- guarantee discovery of every proprietary OEM parameter
- fully scan all 65,536 Mode 22 identifiers on every connection

The application may raw-log unknown positive responses without knowing what the bytes mean.

---

# 4. Critical Concept: Discovery and Decoding Are Different

The application must separate these concepts:

## Discovery

Determining whether this request:

```text
22 1E1C
```

produces a valid response.

## Decoding

Knowing that some byte or bit inside the response means:

```text
Transmission Fluid Temperature
```

and knowing its:

- byte position
- bit position
- length
- endianness
- signedness
- scale
- offset
- unit
- invalid values

A request can therefore be:

```text
SUPPORTED_RAW
```

without being:

```text
SUPPORTED_DECODED
```

Both are useful.

Unknown responses should be preserved.

---

# 5. Terminology

Use the following terminology consistently in code.

## Service

Diagnostic service byte.

Examples:

```text
0x21
0x22
```

## Local Identifier

For Service 21, normally a one-byte identifier:

```text
21 XX
```

Example:

```text
21 82
```

## DID

Data Identifier.

For Service 22, normally a two-byte identifier:

```text
22 XXXX
```

Example:

```text
22 1E1C
```

## Request

The complete diagnostic operation sent to an ECU.

## Signal

An individual interpreted value contained inside a request response.

One request may produce many signals.

## ECU

A specific controller/address pair.

For CAN this could conceptually be:

```text
TX 7E0
RX 7E8
```

The system must NOT assume these addresses universally.

## Enhanced Profile

The learned/cached collection of supported enhanced requests and signal definitions for a specific ECU/vehicle fingerprint.

---

# 6. Architecture

Use the following conceptual architecture:

```text
Existing OBD Connection Layer
        |
        v
Vehicle / ECU Fingerprinting
        |
        v
Enhanced Profile Cache
        |
        +------ cache hit ------+
        |                       |
        v                       v
Quick Validation           Discovery Engine
                                |
                     +----------+----------+
                     |                     |
                     v                     v
                Mode 21 Engine       Mode 22 Engine
                     |                     |
                     +----------+----------+
                                |
                                v
                       Supported Requests
                                |
                     +----------+----------+
                     |                     |
                     v                     v
                Raw Responses        Known Decoders
                     |                     |
                     +----------+----------+
                                |
                                v
                        Request Optimizer
                                |
                     +----------+----------+
                     |                     |
                     v                     v
                 Benchmarking       Multi-DID Tests
                     |                     |
                     +----------+----------+
                                |
                                v
                         Poll Scheduler
                                |
                                v
                             Logger
```

---

# 7. Core Design Rule

Mode 21 and Mode 22 must use different discovery strategies.

## Mode 21

Identifier space:

```text
00-FF
```

Maximum:

```text
256 identifiers
```

A complete scan can therefore be practical.

## Mode 22

Identifier space:

```text
0000-FFFF
```

Maximum:

```text
65,536 identifiers
```

A complete scan is NOT appropriate during normal startup.

Therefore:

```text
Mode 21 = direct scan is acceptable

Mode 22 = profile-driven + ranked discovery + optional background exploration
```

---

# 8. Required Status Model

Never represent request capability with only a boolean.

Use states such as:

```cpp
enum class ProbeStatus
{
    UNKNOWN,

    SUPPORTED_RAW,
    SUPPORTED_DECODED,

    NO_RESPONSE,
    INVALID_RESPONSE,

    SERVICE_UNSUPPORTED,
    IDENTIFIER_UNSUPPORTED,

    SESSION_REQUIRED,
    SECURITY_REQUIRED,

    CONDITIONS_NOT_CORRECT,
    BUSY,
    RESPONSE_PENDING,

    TEMPORARILY_UNAVAILABLE,

    DISABLED
};
```

The system needs to distinguish permanent failures from temporary failures.

---

# 9. Core Data Model

Recommended conceptual structure:

```cpp
enum class EnhancedService
{
    MODE_21 = 0x21,
    MODE_22 = 0x22
};
```

```cpp
struct EcuAddress
{
    uint32_t txAddress;
    uint32_t rxAddress;

    bool extendedAddressing;
};
```

```cpp
struct SignalDefinition
{
    std::string id;
    std::string name;

    size_t bitOffset;
    size_t bitLength;

    bool isSigned;
    bool littleEndian;

    double scale;
    double offset;

    std::string unit;

    double preferredRateHz;
    int priority;
};
```

```cpp
struct EnhancedRequest
{
    EnhancedService service;

    uint16_t identifier;

    EcuAddress ecu;

    ProbeStatus status;

    int responseDataLength;

    std::vector<uint8_t> lastResponse;

    double averageLatencyMs;
    double minimumLatencyMs;
    double maximumLatencyMs;

    int timeoutMs;

    bool requiresSession;
    bool requiresSecurity;

    bool supportsGroupedRequest;

    std::vector<SignalDefinition> signals;
};
```

```cpp
struct EcuEnhancedProfile
{
    EcuAddress address;

    std::string ecuFingerprint;

    std::vector<EnhancedRequest> mode21;
    std::vector<EnhancedRequest> mode22;
};
```

```cpp
struct VehicleEnhancedProfile
{
    std::string vehicleFingerprint;

    std::vector<EcuEnhancedProfile> ecus;

    uint64_t createdTimestamp;
    uint64_t lastValidatedTimestamp;

    int schemaVersion;
};
```

---

# 10. Vehicle and ECU Fingerprinting

Enhanced discoveries must be cached.

The cache must not be matched only by VIN.

Use as much identifying information as the existing system can reliably obtain.

Possible fingerprint inputs:

```text
VIN
physical communication protocol
ECU request address
ECU response address
calibration ID
software ID
ECU identification response
Mode 01 capability bitmap
manufacturer
model/platform when known
```

Conceptually:

```cpp
vehicleFingerprint =
    hash(
        vin +
        protocol +
        ecuAddresses +
        calibrationIds
    );
```

An ECU fingerprint may be:

```cpp
ecuFingerprint =
    hash(
        vehicleFingerprint +
        txAddress +
        rxAddress +
        calibrationId +
        softwareId
    );
```

Do not require every field.

Build the strongest stable fingerprint available.

---

# 11. Fast Startup Flow

Normal startup should follow:

```text
Existing connection established
        |
        v
Determine vehicle fingerprint
        |
        v
Check enhanced cache
        |
    +---+---+
    |       |
   HIT     MISS
    |       |
    v       v
Validate   Initial discovery
cache
    |       |
    +---+---+
        |
        v
Begin logging
        |
        v
Optional background discovery
```

The logger should start as soon as a useful validated logging set exists.

Do not wait for every possible discovery operation.

---

# 12. Cache-Hit Behavior

When an enhanced profile exists:

1. Load cached requests.
2. Select 2-5 representative known-good enhanced requests.
3. Probe them.
4. Confirm expected ECU responses.
5. Confirm response formats/lengths where appropriate.
6. If validation succeeds, immediately begin logging.
7. Continue optional profile refresh later.

Example:

```cpp
bool validateCachedProfile(EcuEnhancedProfile& profile)
{
    auto requests = selectValidationRequests(profile, 3);

    int successCount = 0;

    for (auto& request : requests)
    {
        auto result = probe(request);

        if (isPositiveResponse(result))
            successCount++;
    }

    return successCount >= requiredValidationThreshold;
}
```

If the cached profile no longer validates:

```text
invalidate/profile downgrade
        |
        v
run discovery
```

Do not permanently erase the previous profile immediately; versions may be useful for diagnostics.

---

# 13. Universal Probe Interface

Every enhanced request should pass through one request/probe interface.

Conceptual API:

```cpp
ProbeResult probeEnhanced(
    const EcuAddress& ecu,
    EnhancedService service,
    uint16_t identifier,
    int timeoutMs
);
```

The higher-level discovery engines must not directly depend on serial formatting.

The transport layer handles OBDLink formatting.

---

# 14. OBDLink SX Request Strategy

When possible, use OBDLink commands that allow:

- explicit request header
- explicit timeout
- expected response count

Conceptually:

Mode 21:

```text
STPX h:<TX>,d:21XX,r:1,t:<timeout>
```

Mode 22:

```text
STPX h:<TX>,d:22XXXX,r:1,t:<timeout>
```

Example:

```text
STPX h:7E0,d:2182,r:1,t:15
```

Example:

```text
STPX h:7E0,d:221E1C,r:1,t:15
```

The exact adapter command generation belongs in the adapter transport layer.

The discovery engine should operate on binary diagnostic messages.

---

# 15. Response Handling

The response parser must understand:

## Positive response

Common diagnostic behavior is:

```text
response service = request service + 0x40
```

Therefore:

```text
21 -> 61
22 -> 62
```

Mode 21 example:

```text
Request:
21 82

Positive response:
61 82 AA BB CC DD
```

Mode 22 example:

```text
Request:
22 1E 1C

Positive response:
62 1E 1C AA BB
```

---

# 16. Positive Mode 21 Parser

Conceptually:

```cpp
bool parseMode21Positive(
    uint8_t requestedId,
    const std::vector<uint8_t>& response,
    std::vector<uint8_t>& data)
{
    if (response.size() < 2)
        return false;

    if (response[0] != 0x61)
        return false;

    if (response[1] != requestedId)
        return false;

    data.assign(
        response.begin() + 2,
        response.end()
    );

    return true;
}
```

---

# 17. Positive Mode 22 Parser

Conceptually:

```cpp
bool parseMode22Positive(
    uint16_t requestedDid,
    const std::vector<uint8_t>& response,
    std::vector<uint8_t>& data)
{
    if (response.size() < 3)
        return false;

    if (response[0] != 0x62)
        return false;

    uint16_t returnedDid =
        (uint16_t(response[1]) << 8) |
         uint16_t(response[2]);

    if (returnedDid != requestedDid)
        return false;

    data.assign(
        response.begin() + 3,
        response.end()
    );

    return true;
}
```

---

# 18. Negative Responses

Negative diagnostic responses commonly use:

```text
7F <requested service> <negative response code>
```

Example:

```text
7F 22 31
```

The system must parse these rather than treating all failures as `NO DATA`.

Conceptual parser:

```cpp
bool parseNegativeResponse(
    EnhancedService expectedService,
    const std::vector<uint8_t>& response,
    uint8_t& nrc)
{
    if (response.size() < 3)
        return false;

    if (response[0] != 0x7F)
        return false;

    if (response[1] != static_cast<uint8_t>(expectedService))
        return false;

    nrc = response[2];

    return true;
}
```

---

# 19. Important Negative Response Classification

Recommended handling:

```cpp
ProbeStatus classifyNrc(uint8_t nrc)
{
    switch (nrc)
    {
        case 0x11:
            return ProbeStatus::SERVICE_UNSUPPORTED;

        case 0x21:
            return ProbeStatus::BUSY;

        case 0x22:
            return ProbeStatus::CONDITIONS_NOT_CORRECT;

        case 0x31:
            return ProbeStatus::IDENTIFIER_UNSUPPORTED;

        case 0x33:
            return ProbeStatus::SECURITY_REQUIRED;

        case 0x78:
            return ProbeStatus::RESPONSE_PENDING;

        case 0x7E:
        case 0x7F:
            return ProbeStatus::SESSION_REQUIRED;

        default:
            return ProbeStatus::TEMPORARILY_UNAVAILABLE;
    }
}
```

Exact semantics may vary across protocol generations and OEM implementations.

The important engineering rule is:

> Do not classify all negative responses as unsupported.

---

# 20. Logging-Only Safety Policy

The enhanced discovery engine should remain conservative.

Default behavior:

```text
positive response
    -> accept

identifier unsupported
    -> cache unsupported

service unsupported
    -> stop scanning that service on that ECU

security required
    -> skip

session required
    -> skip unless known profile explicitly permits it

busy
    -> retry later

conditions not correct
    -> mark conditional and retry later if useful

response pending
    -> continue waiting according to diagnostic semantics
```

The generic discovery system must NOT automatically attempt:

```text
10 xx
```

session changes or:

```text
27 xx
```

security access.

Those should only occur when an explicit trusted ECU profile requires them.

---

# 21. Mode 21 Discovery Algorithm

Mode 21 discovery may scan all 256 possible local identifiers.

Pseudo-code:

```cpp
void discoverMode21(EcuEnhancedProfile& ecu)
{
    for (uint16_t id = 0x00; id <= 0xFF; id++)
    {
        ProbeResult result =
            probeEnhanced(
                ecu.address,
                EnhancedService::MODE_21,
                id,
                ecuProbeTimeout(ecu)
            );

        processMode21Probe(
            ecu,
            uint8_t(id),
            result
        );
    }
}
```

---

# 22. Mode 21 Probe Processing

```cpp
void processMode21Probe(
    EcuEnhancedProfile& ecu,
    uint8_t id,
    const ProbeResult& result)
{
    EnhancedRequest request;

    request.service =
        EnhancedService::MODE_21;

    request.identifier = id;
    request.ecu = ecu.address;

    if (result.isPositive)
    {
        request.status =
            ProbeStatus::SUPPORTED_RAW;

        request.responseDataLength =
            result.data.size();

        request.lastResponse =
            result.data;

        request.averageLatencyMs =
            result.elapsedMs;

        attachKnownDecoder(request);

        if (!request.signals.empty())
            request.status =
                ProbeStatus::SUPPORTED_DECODED;

        ecu.mode21.push_back(request);

        return;
    }

    request.status =
        classifyProbeFailure(result);

    storeProbeKnowledge(request);
}
```

---

# 23. Why Mode 21 Is Important for Logging

One Mode 21 request may return an entire OEM-defined data block.

Example:

```text
21 82
```

might return:

```text
61 82
AA BB
CC DD
EE FF
11 22
```

Those bytes may contain several independent signals.

Therefore the database model must allow:

```text
one request
    -> many signals
```

Do NOT architect the system as:

```text
one diagnostic request
    =
one logged channel
```

That assumption would significantly reduce logging efficiency.

---

# 24. Mode 21 Decoder Model

Example concept:

```json
{
  "service": "21",
  "identifier": "82",
  "response_data_length": 8,
  "signals": [
    {
      "name": "SignalA",
      "bit_offset": 0,
      "bit_length": 16,
      "signed": false,
      "endian": "big",
      "scale": 0.1,
      "offset": -40,
      "unit": "C"
    },
    {
      "name": "SignalB",
      "bit_offset": 16,
      "bit_length": 16,
      "signed": false,
      "endian": "big",
      "scale": 1.0,
      "offset": 0,
      "unit": "rpm"
    }
  ]
}
```

---

# 25. Mode 22 Discovery Strategy

Normal Mode 22 discovery should have multiple layers.

Priority:

```text
1. Exact cached ECU profile
2. Exact known calibration/software family
3. Known vehicle/platform DIDs
4. Known manufacturer DIDs
5. Globally common known DIDs
6. User-imported definitions
7. Selected exploration ranges
8. Optional exhaustive exploration
```

The application should usually reach useful data before reaching level 7.

---

# 26. Mode 22 Candidate Structure

```cpp
struct DidCandidate
{
    uint16_t did;

    int priority;

    std::string source;

    std::optional<std::string> manufacturer;
    std::optional<std::string> platform;
    std::optional<std::string> ecuFamily;
    std::optional<std::string> calibrationPattern;

    uint64_t historicalAttempts;
    uint64_t historicalSuccesses;

    std::vector<SignalDefinition> knownSignals;
};
```

---

# 27. Mode 22 Candidate Queue

Use a priority queue rather than scanning numerically.

```cpp
std::priority_queue<DidCandidate>
buildDidQueue(
    const VehicleContext& vehicle,
    const EcuContext& ecu);
```

Conceptual scoring:

```cpp
double scoreDidCandidate(
    const DidCandidate& candidate,
    const VehicleContext& vehicle,
    const EcuContext& ecu)
{
    double score = 0;

    if (exactCalibrationMatch(candidate, ecu))
        score += 100000;

    if (exactEcuFamilyMatch(candidate, ecu))
        score += 50000;

    if (exactPlatformMatch(candidate, vehicle))
        score += 30000;

    if (manufacturerMatch(candidate, vehicle))
        score += 10000;

    if (!candidate.knownSignals.empty())
        score += 20000;

    double successRate =
        candidate.historicalAttempts > 0
        ? double(candidate.historicalSuccesses) /
          double(candidate.historicalAttempts)
        : 0.0;

    score += successRate * 5000;

    return score;
}
```

Probe highest-value candidates first.

---

# 28. Mode 22 Discovery Loop

```cpp
void discoverMode22(
    VehicleContext& vehicle,
    EcuEnhancedProfile& ecu)
{
    auto queue =
        buildDidQueue(
            vehicle,
            ecu
        );

    while (!queue.empty())
    {
        auto candidate =
            queue.top();

        queue.pop();

        if (discoveryBudgetExpired())
            break;

        auto result =
            probeEnhanced(
                ecu.address,
                EnhancedService::MODE_22,
                candidate.did,
                ecuProbeTimeout(ecu)
            );

        processMode22Probe(
            ecu,
            candidate,
            result
        );
    }
}
```

Normal startup should have a discovery time/request budget.

Example:

```text
maximum startup enhanced discovery:
100 requests

or

500 ms

whichever occurs first
```

Exact values should be configurable.

---

# 29. Mode 22 Probe Processing

```cpp
void processMode22Probe(
    EcuEnhancedProfile& ecu,
    const DidCandidate& candidate,
    const ProbeResult& result)
{
    updateDidStatistics(
        candidate.did,
        result
    );

    if (!result.isPositive)
    {
        storeNegativeKnowledge(
            ecu,
            candidate,
            result
        );

        return;
    }

    EnhancedRequest request;

    request.service =
        EnhancedService::MODE_22;

    request.identifier =
        candidate.did;

    request.ecu =
        ecu.address;

    request.status =
        ProbeStatus::SUPPORTED_RAW;

    request.responseDataLength =
        result.data.size();

    request.lastResponse =
        result.data;

    request.averageLatencyMs =
        result.elapsedMs;

    attachKnownDecoder(request);

    if (!request.signals.empty())
    {
        request.status =
            ProbeStatus::SUPPORTED_DECODED;
    }

    ecu.mode22.push_back(request);
}
```

---

# 30. Unknown Positive Mode 22 Responses

If:

```text
22 A1 23
```

returns:

```text
62 A1 23 10 94 01 FF 32
```

but no decoder exists:

Store:

```text
DID: A123
supported: true
payload length: 5
raw bytes: 10 94 01 FF 32
decoder: unknown
```

The logger should be capable of logging:

```text
timestamp
ECU
service
identifier
raw payload
```

Example:

```text
18453.330,7E8,22,A123,109401FF32
```

This allows future decoding without losing previously captured data.

---

# 31. Global Learning Database

The application should accumulate anonymous technical capability statistics locally or in whatever project database architecture exists.

Conceptual model:

```cpp
struct DidStatistics
{
    uint16_t did;

    uint64_t attempts;
    uint64_t positiveResponses;

    std::unordered_map<
        std::string,
        uint64_t
    > successByManufacturer;

    std::unordered_map<
        std::string,
        uint64_t
    > successByEcuFamily;
};
```

This allows future unknown vehicles to prioritize DIDs that commonly succeed.

Example:

```text
DID    Ford success    GM success    Toyota success
1E1C   very high       low           none
1234   medium          high          low
...
```

The database becomes more effective over time.

---

# 32. Startup Discovery Versus Background Discovery

The discovery system should have two modes.

## Startup Discovery

Goal:

> Find enough enhanced signals to begin useful logging immediately.

Characteristics:

- small request budget
- high-confidence candidates only
- known decoders prioritized
- minimal latency

## Background Discovery

Goal:

> Learn additional supported enhanced identifiers without hurting logging.

Characteristics:

- low priority
- only runs when scheduler has spare bandwidth
- stops when logging demand rises
- may explore lower-confidence candidates

---

# 33. Background Discovery Scheduler

Conceptually:

```cpp
void maybeRunBackgroundDiscovery()
{
    if (!logger.isActive())
        return;

    if (scheduler.highPriorityJobDueSoon())
        return;

    if (scheduler.busUtilization() >
        maxDiscoveryUtilization)
        return;

    if (!scheduler.hasIdleWindow(
            minimumDiscoveryWindowMs))
        return;

    probeNextBackgroundCandidate();
}
```

Recommended rule:

> Logging always wins over discovery.

---

# 34. Optional Full Mode 22 Exploration

Full:

```text
0000-FFFF
```

scanning should only happen when explicitly enabled.

Possible reasons:

- engineering/development mode
- building a new ECU profile
- bench testing
- reverse-engineering workflow

It should NOT be default runtime behavior.

The scan should support:

```text
pause
resume
checkpoint
range selection
ECU selection
rate limits
results persistence
```

Never restart a 65,536-DID scan from zero unnecessarily.

---

# 35. Range-Based Mode 22 Exploration

Support explicit ranges:

```cpp
struct DidRange
{
    uint16_t start;
    uint16_t end;

    int priority;
};
```

Example:

```text
1000-10FF
1800-18FF
1E00-1EFF
F100-F1FF
```

The ranges should come from:

- profile knowledge
- imported definitions
- OEM-specific knowledge
- developer configuration

Do not assume a particular range universally.

---

# 36. Adaptive Timeout System

Default adapter timeouts can waste enormous amounts of time during discovery.

Measure each ECU.

Use an already known-good request.

Example:

```cpp
std::vector<double>
measureResponseLatency(
    DiagnosticRequest request,
    int sampleCount);
```

Calculate:

```text
median
P95
P99
maximum
```

Then:

```cpp
timeoutMs =
    clamp(
        ceil(P99 * multiplier + margin),
        minimumTimeoutMs,
        maximumTimeoutMs
    );
```

Possible starting policy for CAN:

```text
multiplier = 2.0
margin = 2-5 ms
minimum = 10 ms
maximum = 50 ms
```

This is only an initial engineering policy.

Actual values should be measured and configurable.

Legacy protocols may require substantially larger timeouts.

Never use CAN timing assumptions for:

```text
ISO 9141
KWP over K-Line
J1850
```

---

# 37. Timeout Must Be Per ECU

Different ECUs can respond at different speeds.

Maintain:

```cpp
struct EcuTimingProfile
{
    double medianMs;
    double p95Ms;
    double p99Ms;

    int normalTimeoutMs;
    int discoveryTimeoutMs;
};
```

Do not apply one timeout globally.

---

# 38. Request Benchmarking

After discovering useful requests, benchmark them.

For each request:

```cpp
void benchmarkRequest(
    EnhancedRequest& request,
    int samples);
```

Record:

```text
average response latency
min latency
max latency
payload bytes
multi-frame frequency
error rate
timeout rate
```

Example:

```text
Request     Avg      Data bytes
2101        4.2 ms       14
2182        4.5 ms        8
221234      3.8 ms        2
221235      3.7 ms        2
```

---

# 39. Request Efficiency

Calculate approximately:

```cpp
requestEfficiency =
    usefulSignalCount /
    averageLatencyMs;
```

Example:

```text
2182:
8 useful signals / 4.5 ms
= 1.78 signals/ms

221234:
1 useful signal / 3.8 ms
= 0.26 signals/ms
```

This information should influence scheduling.

A request that yields ten useful channels can be much more valuable than ten individual single-value requests.

---

# 40. Polling Scheduler

Enhanced data should use a priority/deadline scheduler.

Conceptual:

```cpp
struct PollJob
{
    DiagnosticRequest request;

    double requestedHz;
    double effectiveHz;

    int priority;

    double nextDueTimestamp;

    std::vector<SignalDefinition> signals;
};
```

Scheduler:

```cpp
while (logging)
{
    PollJob* job =
        selectHighestPriorityDueJob();

    if (!job)
    {
        maybeRunBackgroundDiscovery();
        continue;
    }

    auto result =
        sendAndReceive(job->request);

    decodeAndLog(
        *job,
        result
    );

    updateRequestStatistics(
        *job,
        result
    );

    scheduleNextExecution(*job);
}
```

---

# 41. Signal Rate Classes

Useful default categories:

## Very Fast

Examples:

```text
RPM
engine torque related values
throttle
MAP/boost
ignition timing
lambda
fuel pressure
vehicle/driveshaft speed
```

Possible target:

```text
20-100+ Hz
```

when physically achievable.

## Medium

Examples:

```text
fuel trims
air temperature
load
transmission state
```

Possible target:

```text
5-20 Hz
```

## Slow

Examples:

```text
coolant temperature
oil temperature
transmission temperature
battery voltage
ambient temperature
fuel level
```

Possible target:

```text
1-5 Hz
```

These are scheduling categories, not guaranteed rates.

---

# 42. Effective Poll Rate

Requested rates may exceed what the ECU/transport can deliver.

Therefore:

```cpp
effectiveHz =
    min(
        desiredHz,
        measuredAvailableRate
    );
```

The scheduler should degrade gracefully.

High-priority channels should retain rate before low-priority channels.

---

# 43. Mode 22 Multi-DID Requests

Once individual DIDs are known, the application may test whether the ECU supports retrieving multiple DIDs in one Service 22 request.

Individual:

```text
22 12 34
22 12 35
22 12 36
```

Potential grouped request:

```text
22 12 34 12 35 12 36
```

Potential response:

```text
62
12 34 <data>
12 35 <data>
12 36 <data>
```

Only attempt this after:

1. all DIDs are individually validated
2. expected response lengths are known
3. response parser can safely separate the returned records

---

# 44. Multi-DID Discovery Policy

Do NOT use multi-DID requests to discover completely unknown DIDs.

Use:

```text
one unknown DID per discovery request
```

because unknown response record lengths make generic parsing unreliable.

Use:

```text
multi-DID only for already-known DIDs
```

---

# 45. Multi-DID Optimizer

Conceptual:

```cpp
bool testMultiDidGroup(
    const EcuAddress& ecu,
    std::vector<EnhancedRequest*>& requests)
{
    for (auto* request : requests)
    {
        if (request->service !=
            EnhancedService::MODE_22)
            return false;

        if (request->responseDataLength <= 0)
            return false;
    }

    auto combined =
        buildMultiDidRequest(requests);

    auto response =
        sendDiagnosticRequest(
            ecu,
            combined
        );

    if (!validateGroupedResponse(
            response,
            requests))
    {
        return false;
    }

    storeOptimizedGroup(requests);

    return true;
}
```

---

# 46. Group Size Testing

Do not immediately combine every known DID.

Test incrementally:

```text
2 DIDs
then 3
then 4
...
```

Stop when:

```text
negative response
timeout
response becomes unreliable
payload size becomes excessive
latency becomes worse than separate requests
```

Optimization should be empirical.

---

# 47. Request Group Representation

```cpp
struct RequestGroup
{
    EcuAddress ecu;

    std::vector<uint16_t> dids;

    double averageLatencyMs;

    int totalSignalCount;

    bool validated;
};
```

Scheduler treats a successful group as one poll job.

---

# 48. Decoder Database Architecture

Definitions should live outside application logic.

Suggested structure:

```text
definitions/
    ford/
        pcm/
        tcm/

    gm/
        ecm/
        tcm/

    toyota/
        ecm/
        tcm/

    generic/
```

Or database tables/JSON equivalent.

Do not hard-code OEM equations into the scheduler.

---

# 49. Example Definition

```json
{
  "manufacturer": "ExampleOEM",
  "ecu_family": "ExampleECU",
  "service": "22",
  "identifier": "1234",

  "response_length": 4,

  "signals": [
    {
      "name": "ExampleValue1",
      "bit_offset": 0,
      "bit_length": 16,
      "signed": false,
      "endian": "big",
      "scale": 0.1,
      "offset": 0,
      "unit": "kPa",
      "preferred_hz": 50
    },

    {
      "name": "ExampleValue2",
      "bit_offset": 16,
      "bit_length": 16,
      "signed": true,
      "endian": "big",
      "scale": 0.01,
      "offset": 0,
      "unit": "unit",
      "preferred_hz": 20
    }
  ]
}
```

---

# 50. Profile Matching

Definitions may specify matching constraints:

```json
{
  "match": {
    "manufacturer": "ExampleOEM",
    "ecu_family": "ABC",
    "calibration_prefix": "XYZ",
    "protocol": "CAN"
  }
}
```

Match strongest definitions first.

Priority:

```text
exact calibration
exact ECU family
exact platform
manufacturer
generic
```

---

# 51. Raw Logging Format

Always permit raw request logging.

Recommended minimum fields:

```text
timestamp
vehicle fingerprint
ECU TX
ECU RX
service
identifier
request bytes
response bytes
response latency
status
```

Example:

```csv
timestamp,tx,rx,service,id,response,latency_ms,status
18453.330,7E0,7E8,22,A123,109401FF32,4.22,SUPPORTED_RAW
```

Decoded signals can be logged separately or alongside raw data.

---

# 52. Why Raw Logging Is Mandatory

A currently unknown DID may be decoded later.

If the raw bytes are retained, old logs can potentially be reprocessed with future decoder definitions.

Therefore:

> Never discard a successful unknown enhanced response just because its meaning is unknown.

---

# 53. Discovery Database

Store discovered capabilities separately from decoded definitions.

For example:

```text
capabilities
    vehicle fingerprint
    ECU fingerprint
    service
    identifier
    positive/negative
    payload length
    latency
    last seen
```

Definitions:

```text
signal_definitions
    OEM
    ECU family
    service
    identifier
    byte layout
    formulas
```

These are distinct concepts.

---

# 54. Persistent Cache Example

```json
{
  "schema_version": 1,

  "vehicle_fingerprint": "abc123",

  "ecus": [
    {
      "tx": "7E0",
      "rx": "7E8",

      "ecu_fingerprint": "def456",

      "timing": {
        "median_ms": 3.8,
        "p99_ms": 6.1,
        "discovery_timeout_ms": 15
      },

      "mode21": [
        {
          "id": "01",
          "status": "SUPPORTED_DECODED",
          "response_length": 14,
          "avg_latency_ms": 4.1
        },

        {
          "id": "82",
          "status": "SUPPORTED_RAW",
          "response_length": 8,
          "avg_latency_ms": 4.4
        }
      ],

      "mode22": [
        {
          "id": "1E1C",
          "status": "SUPPORTED_DECODED",
          "response_length": 2,
          "avg_latency_ms": 3.9
        }
      ]
    }
  ]
}
```

---

# 55. Negative Capability Cache

Cache negative results too.

Example:

```json
{
  "service": "22",
  "id": "4321",
  "status": "IDENTIFIER_UNSUPPORTED",
  "last_checked": 1760000000
}
```

This prevents repeatedly wasting time on known unsupported DIDs.

However negative entries should support expiration/versioning because ECU software may change.

---

# 56. Cache Expiration

Positive results:

```text
long-lived
```

Negative results:

```text
long-lived but version-aware
```

Invalidate or reconsider when:

```text
calibration changes
ECU software ID changes
ECU address changes
vehicle fingerprint changes
user requests rescan
definition database changes substantially
```

---

# 57. Discovery Budget

Every discovery phase should have explicit limits.

Example:

```cpp
struct DiscoveryBudget
{
    int maxRequests;
    double maxDurationMs;
    double maxBusUtilization;
};
```

Startup example:

```text
max requests: 50-200
max duration: 250-1000 ms
```

Background example:

```text
unlimited duration
strict utilization limit
logging always higher priority
```

Exact defaults should be tuned experimentally.

---

# 58. Mode 21 Startup Policy

If no cache exists:

Option A:

```text
scan all 256 IDs before enhanced logging
```

Option B, preferred:

```text
probe known high-value IDs
start logging
continue remaining 21 scan in background
```

For maximum startup speed, prefer B.

---

# 59. Mode 22 Startup Policy

Never normal-startup scan all DIDs.

Use:

```text
known exact profile
        |
known OEM candidates
        |
known global candidates
        |
start logging
        |
background exploration
```

---

# 60. Discovery Priority Formula

A candidate's priority can combine:

```text
known decoder
exact ECU match
exact calibration match
manufacturer match
historical success rate
expected signal value
expected request efficiency
```

Conceptually:

```cpp
priority =
    decoderScore +
    profileMatchScore +
    historicalSuccessScore +
    loggingValueScore;
```

---

# 61. Logging Value Score

If a request has a known decoder:

```cpp
loggingValue =
    sum(signalPriorities) /
    expectedLatencyMs;
```

This encourages block requests that return many important signals efficiently.

---

# 62. Request Failure During Logging

Logging requests may fail temporarily.

Do not remove them after one failure.

Track health:

```cpp
struct RequestHealth
{
    uint64_t successes;
    uint64_t failures;

    int consecutiveFailures;

    double successRate;
};
```

Policy example:

```text
1 failure
    -> retry normally

3 consecutive failures
    -> back off

10 failures
    -> disable temporarily

successful retry
    -> restore
```

---

# 63. Dynamic Backoff

Use exponential or staged backoff for failing requests.

Example:

```text
normal: every 20 ms

after repeated failures:
100 ms
500 ms
2 sec
10 sec
```

Do not allow one broken enhanced request to destroy logging throughput.

---

# 64. ECU Isolation

Keep enhanced capabilities per ECU.

Never assume a DID supported by:

```text
7E8
```

is supported by:

```text
7E9
```

Even identical identifiers may mean different things on different controllers.

Key should include:

```text
ECU identity
service
identifier
```

not just:

```text
identifier
```

---

# 65. Functional Versus Physical Requests

Discovery and high-speed logging should prefer physical ECU addressing once the target ECU is known.

Reason:

- predictable responder
- reduced ambiguity
- less waiting for multiple responses
- easier timeout optimization
- cleaner caching

Functional addressing may still be useful during initial ECU identification.

---

# 66. Transport Independence

The enhanced layer should not care whether the vehicle uses:

```text
CAN
KWP
ISO9141
J1850
```

Use an abstraction:

```cpp
class DiagnosticTransport
{
public:

    virtual DiagnosticResponse send(
        const EcuAddress& ecu,
        const std::vector<uint8_t>& request,
        int timeoutMs
    ) = 0;
};
```

The enhanced engine only sees diagnostic payloads.

---

# 67. Main Enhanced Manager

Conceptual:

```cpp
class EnhancedLoggingManager
{
public:

    void initialize(
        const VehicleContext& vehicle);

    void discoverStartupCapabilities();

    void startLogging();

    void runBackgroundDiscovery();

    void stop();

private:

    void loadProfile();
    bool validateProfile();

    void discoverMode21();
    void discoverMode22Candidates();

    void benchmarkRequests();
    void optimizeRequestGroups();

    void buildPollSchedule();

    void persistProfile();
};
```

---

# 68. Recommended Main Startup Implementation

```cpp
void EnhancedLoggingManager::initialize(
    const VehicleContext& vehicle)
{
    vehicle_ = vehicle;

    loadProfile();

    if (profileExists())
    {
        if (validateProfile())
        {
            benchmarkCriticalRequests();
            buildPollSchedule();
            startLogging();

            enableBackgroundRefresh();

            return;
        }
    }

    discoverHighPriorityDefinitions();

    discoverHighProbabilityMode21();

    discoverHighProbabilityMode22();

    buildPollSchedule();

    startLogging();

    enableBackgroundDiscovery();
}
```

Important:

> Logging begins before exhaustive discovery finishes.

---

# 69. Background Worker Concept

```cpp
void EnhancedLoggingManager::
runBackgroundDiscovery()
{
    while (loggingActive())
    {
        if (!schedulerCanSpareBandwidth())
        {
            sleepBriefly();
            continue;
        }

        if (hasUnscannedMode21())
        {
            probeNextMode21();
            continue;
        }

        if (hasMode22Candidates())
        {
            probeNextMode22Candidate();
            continue;
        }

        if (developerExplorationEnabled())
        {
            probeNextExplorationDid();
            continue;
        }

        sleepBriefly();
    }
}
```

---

# 70. Scheduler / Discovery Coordination

Do not run separate workers that blindly send diagnostic commands concurrently through the same ECU connection.

Centralize all traffic through one arbitration system.

Example:

```text
Diagnostic Request Arbiter
        |
        +-- urgent logging
        |
        +-- normal logging
        |
        +-- slow logging
        |
        +-- cache validation
        |
        +-- background discovery
```

Only one request owns the adapter/diagnostic exchange at a time unless the transport implementation explicitly supports something more advanced.

---

# 71. Request Arbiter

Conceptual priorities:

```cpp
enum class RequestPriority
{
    LOG_CRITICAL = 100,
    LOG_FAST     = 80,
    LOG_NORMAL   = 60,
    LOG_SLOW     = 40,
    VALIDATION   = 30,
    DISCOVERY    = 10
};
```

The arbiter chooses the next request.

---

# 72. Logging Should Never Wait for Discovery

This is a hard rule.

If:

```text
RPM request due now
```

and:

```text
unknown DID discovery request waiting
```

send RPM.

Discovery waits.

---

# 73. Agent Implementation Rules

Any coding agent working on this subsystem should follow these rules.

## Rule 1

Do not modify existing standard OBD-II protocol detection unless necessary.

This document covers enhanced Mode 21/22 behavior.

## Rule 2

Do not assume fixed CAN IDs such as `7E0/7E8`.

Use ECU addresses discovered by the existing application.

## Rule 3

Do not treat Mode 21 and Mode 22 as standard PID bitmaps.

There is no universal support bitmap equivalent to Mode 01 PID `00`.

## Rule 4

Mode 21 may be exhaustively scanned because its local-ID space is small.

## Rule 5

Mode 22 should normally use ranked candidate discovery.

## Rule 6

Never brute-force 65,536 Mode 22 requests during ordinary startup.

## Rule 7

Store positive raw responses even when no decoder is known.

## Rule 8

Cache both positive and useful negative discovery results.

## Rule 9

Cache per vehicle + ECU fingerprint.

## Rule 10

Always separate transport, discovery, decoder, scheduler, and logging code.

## Rule 11

One request may decode into multiple signals.

## Rule 12

Measure request latency instead of assuming fixed performance.

## Rule 13

Logging has higher priority than discovery.

## Rule 14

Do not automatically request diagnostic security access.

## Rule 15

Do not automatically enter non-default sessions unless a trusted profile specifically requires it.

## Rule 16

Do not clear DTCs or send write/control services as part of discovery.

## Rule 17

Unknown data is still valuable; raw-log it.

## Rule 18

Do not repeatedly rediscover a known vehicle every startup.

Validate cache and reuse it.

## Rule 19

Optimize for useful signals per unit time, not simply requests per second.

---

# 74. Suggested Source Layout

Possible implementation:

```text
src/
    obd/
        adapter/
            obdlink_sx.*

        diagnostic/
            diagnostic_transport.*
            diagnostic_response.*
            negative_response.*

        enhanced/
            enhanced_manager.*
            enhanced_profile.*
            enhanced_request.*
            enhanced_cache.*

            mode21_discovery.*
            mode22_discovery.*

            did_candidate.*
            did_database.*

            enhanced_decoder.*
            signal_definition.*

            request_benchmark.*
            request_optimizer.*
            multi_did_optimizer.*

            poll_scheduler.*
            request_arbiter.*

        logging/
            raw_enhanced_logger.*
            decoded_signal_logger.*
```

Definitions:

```text
data/
    enhanced/
        definitions/
        profiles/
        learned_statistics/
```

---

# 75. Suggested Implementation Order

## Phase 1

Implement core request structures.

```text
EnhancedRequest
ProbeResult
ProbeStatus
EcuEnhancedProfile
VehicleEnhancedProfile
```

## Phase 2

Implement generic `0x21`/`0x22` response parser.

Include:

```text
61 positive
62 positive
7F negative
```

## Phase 3

Implement full Mode 21 scanning.

```text
00-FF
```

Store raw responses.

## Phase 4

Implement Mode 22 candidate queue.

Start with imported known definitions only.

## Phase 5

Implement persistent cache.

## Phase 6

Implement quick cache validation.

## Phase 7

Implement background discovery.

## Phase 8

Implement request benchmarking.

## Phase 9

Implement rate-aware scheduler.

## Phase 10

Implement multi-DID Mode 22 optimization.

## Phase 11

Implement learning statistics.

## Phase 12

Add developer-only range/full DID exploration.

---

# 76. Minimum Viable Version

The first useful implementation does not need every optimization.

MVP:

```text
existing protocol/ECU detection
        |
Mode 21 00-FF scanner
        |
known-list Mode 22 scanner
        |
positive/negative parser
        |
raw logging
        |
decoder lookup
        |
persistent cache
        |
reuse cached profile next startup
```

This alone provides substantial value.

---

# 77. Version 2

Add:

```text
adaptive timeouts
request benchmarking
background discovery
priority scheduler
negative capability cache
```

---

# 78. Version 3

Add:

```text
global DID statistics
OEM/platform candidate ranking
multi-DID 22 optimization
automatic request grouping
request efficiency scoring
profile confidence system
```

---

# 79. Suggested ProbeResult

```cpp
struct ProbeResult
{
    bool receivedResponse;
    bool isPositive;
    bool isNegative;

    ProbeStatus status;

    uint8_t negativeResponseCode;

    std::vector<uint8_t> rawResponse;
    std::vector<uint8_t> data;

    double elapsedMs;

    bool wasMultiFrame;

    std::string adapterError;
};
```

---

# 80. Central Probe Function

Conceptual implementation:

```cpp
ProbeResult probeEnhanced(
    DiagnosticTransport& transport,
    const EcuAddress& ecu,
    EnhancedService service,
    uint16_t identifier,
    int timeoutMs)
{
    std::vector<uint8_t> request;

    if (service ==
        EnhancedService::MODE_21)
    {
        request.push_back(0x21);
        request.push_back(
            uint8_t(identifier)
        );
    }

    else if (service ==
             EnhancedService::MODE_22)
    {
        request.push_back(0x22);

        request.push_back(
            uint8_t(identifier >> 8)
        );

        request.push_back(
            uint8_t(identifier & 0xFF)
        );
    }

    auto response =
        transport.send(
            ecu,
            request,
            timeoutMs
        );

    return parseEnhancedResponse(
        service,
        identifier,
        response
    );
}
```

---

# 81. Central Response Parser

```cpp
ProbeResult parseEnhancedResponse(
    EnhancedService service,
    uint16_t identifier,
    const DiagnosticResponse& response)
{
    ProbeResult result;

    result.rawResponse =
        response.payload;

    result.elapsedMs =
        response.elapsedMs;

    if (response.payload.empty())
    {
        result.status =
            ProbeStatus::NO_RESPONSE;

        return result;
    }

    const auto& msg =
        response.payload;

    uint8_t expectedPositive =
        uint8_t(service) + 0x40;

    if (msg[0] == expectedPositive)
    {
        result.receivedResponse = true;
        result.isPositive = true;

        if (service ==
            EnhancedService::MODE_21)
        {
            if (msg.size() < 2 ||
                msg[1] !=
                uint8_t(identifier))
            {
                result.status =
                    ProbeStatus::INVALID_RESPONSE;

                return result;
            }

            result.data.assign(
                msg.begin() + 2,
                msg.end()
            );
        }

        else
        {
            if (msg.size() < 3)
            {
                result.status =
                    ProbeStatus::INVALID_RESPONSE;

                return result;
            }

            uint16_t returnedDid =
                (uint16_t(msg[1]) << 8) |
                 uint16_t(msg[2]);

            if (returnedDid != identifier)
            {
                result.status =
                    ProbeStatus::INVALID_RESPONSE;

                return result;
            }

            result.data.assign(
                msg.begin() + 3,
                msg.end()
            );
        }

        result.status =
            ProbeStatus::SUPPORTED_RAW;

        return result;
    }

    if (msg.size() >= 3 &&
        msg[0] == 0x7F &&
        msg[1] == uint8_t(service))
    {
        result.receivedResponse = true;
        result.isNegative = true;

        result.negativeResponseCode =
            msg[2];

        result.status =
            classifyNrc(msg[2]);

        return result;
    }

    result.status =
        ProbeStatus::INVALID_RESPONSE;

    return result;
}
```

---

# 82. Test Requirements

Unit tests must cover at least:

## Mode 21 positive

```text
21 82
61 82 AA BB
```

Expected:

```text
SUPPORTED_RAW
data = AA BB
```

## Mode 22 positive

```text
22 12 34
62 12 34 AA BB
```

Expected:

```text
SUPPORTED_RAW
data = AA BB
```

## Unsupported DID

```text
22 12 34
7F 22 31
```

Expected:

```text
IDENTIFIER_UNSUPPORTED
```

## Security required

```text
7F 22 33
```

Expected:

```text
SECURITY_REQUIRED
```

## Response pending

```text
7F 22 78
```

Expected behavior:

```text
wait for final response
```

## Wrong DID echoed

Request:

```text
22 12 34
```

Response:

```text
62 12 35 AA
```

Expected:

```text
INVALID_RESPONSE
```

## Timeout

Expected:

```text
NO_RESPONSE
```

---

# 83. Integration Tests

Test against:

```text
simulated fast CAN ECU
simulated slow CAN ECU
Mode 21 ECU
Mode 22 ECU
ECU with mixed supported/unsupported IDs
ECU requiring session
ECU returning response pending
multi-frame responses
multiple ECUs
cached profile reconnect
changed calibration invalidating cache
```

---

# 84. Performance Metrics

Collect:

```text
time from connection to first logged enhanced value
time to cache validation
Mode 21 IDs scanned/sec
Mode 22 candidates scanned/sec
average request latency
logging samples/sec
decoded signals/sec
adapter utilization
vehicle bus utilization estimate
discovery request count
cache hit rate
```

The most important metric is:

```text
connection -> useful logging
```

not:

```text
connection -> discovery complete
```

---

# 85. Preferred System Behavior Example

First connection:

```text
Vehicle connected
    |
standard detection already works
    |
fingerprint created
    |
no enhanced cache
    |
probe high-value 21/22 definitions
    |
several requests succeed
    |
START LOGGING
    |
remaining Mode 21 scanned in background
    |
additional Mode 22 candidates tested
    |
requests benchmarked
    |
multi-DID groups tested
    |
profile saved
```

Second connection:

```text
Vehicle connected
    |
fingerprint matches
    |
load enhanced cache
    |
validate 3 requests
    |
START LOGGING
    |
optional refresh in background
```

The second connection should be dramatically faster.

---

# 86. Core Philosophy

The application should not attempt to solve enhanced diagnostics by asking:

> What does this entire car support?

Instead ask:

> What useful requests can I prove this ECU supports right now, and how efficiently can I poll them?

That changes the problem from exhaustive enumeration into high-speed capability learning.

---

# 87. Final Behavioral Rules

The finished system should behave according to the following principles:

```text
Mode 01:
use existing standard capability discovery

Mode 21:
scan intelligently, potentially all 256 IDs

Mode 22:
probe ranked candidates, not all 65,536 IDs

Known profile:
validate and immediately reuse

Unknown positive response:
raw-log it

Known decoder:
decode into signals

One request containing many signals:
prefer it where useful

Known DIDs:
test grouping when appropriate

Slow/nonresponsive request:
reduce priority or disable

High-priority logging:
always beats discovery

Discovery:
continues only when bandwidth permits

Security/session-restricted data:
skip unless explicitly profiled

Every successful discovery:
cache it

Every useful negative result:
cache it

Every request:
benchmark it

Every ECU:
maintain separate capabilities

Every future connection:
reuse what was learned
```

---

# 88. Agent Directive

When implementing or modifying this subsystem, preserve the following invariant:

> The purpose of enhanced discovery is to improve logging, not to delay logging.

Any design that waits for exhaustive Mode 21/22 discovery before beginning useful logging should be considered incorrect unless explicitly running in a developer discovery mode.

The preferred architecture is:

```text
identify
validate
log
learn more
optimize
cache
reuse
```

rather than:

```text
scan everything
then log
```

That principle should guide implementation decisions throughout the project.