type TryCandidate = {
  service: string;
  identifier: string;
  request_header: string;
  response_header: string;
  source_blob_sha: string;
  source_url: string | null;
  license: string | null;
  license_url: string | null;
};

export function oemCandidateTryState(
  candidate: TryCandidate,
  device: string,
  blocked: boolean,
  adapter: "obdlink_sx" | "a30" | "elm327" | "openport2" | "simulator" | "candlelight_obd" = "obdlink_sx",
) {
  const supportedAdapter = adapter === "obdlink_sx" || adapter === "a30" || adapter === "elm327"
    || adapter === "openport2" || adapter === "candlelight_obd" || adapter === "simulator";
  if (!supportedAdapter) return { disabled: true, reason: "OEM reads are unavailable for this adapter" };
  if (!device) {
    const reason = adapter === "a30" || adapter === "elm327" ? "Select a paired Bluetooth device"
      : adapter === "openport2" ? "Connect an OpenPort 2 adapter"
        : adapter === "candlelight_obd" ? "Connect a Candlelight OBD adapter"
          : adapter === "simulator" ? "Connect the simulator"
            : "Select an OBDLink SX port";
    return { disabled: true, reason };
  }
  if (blocked) return { disabled: true, reason: "Connect a supported adapter and finish replay or other current actions before testing" };
  const identifierWidth = candidate.service === "21" ? 2 : candidate.service === "22" ? 4 : 0;
  const validHex = (value: string, width: number) => value.length === width && /^[0-9a-f]+$/i.test(value);
  const validCanHeader = (value: string) => validHex(value, 3) && Number.parseInt(value, 16) <= 0x7ff;
  if (adapter === "a30" && validCanHeader(candidate.request_header) && Number.parseInt(candidate.request_header, 16) < 0x700) {
    return { disabled: true, reason: "A30 OEM reads require a 0x7xx request CAN ID" };
  }
  const hasSource = Boolean(candidate.source_blob_sha && candidate.source_url && candidate.license && candidate.license_url);
  const validRequest = identifierWidth > 0
    && validHex(candidate.identifier, identifierWidth)
    && validCanHeader(candidate.request_header)
    && validCanHeader(candidate.response_header)
    && hasSource;
  return validRequest
    ? { disabled: false, reason: "" }
    : { disabled: true, reason: "Candidate lacks a source-backed service 21/22 request and exact 11-bit CAN route" };
}
