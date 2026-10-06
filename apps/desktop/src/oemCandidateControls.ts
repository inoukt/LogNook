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
  adapter: "obdlink_sx" | "a30" | "elm327" | "openport2" | "simulator" = "obdlink_sx",
) {
  if (adapter !== "obdlink_sx" && adapter !== "a30" && adapter !== "elm327") {
    return { disabled: true, reason: "OEM reads require OBDLink SX or Bluetooth ELM327/A30" };
  }
  if (!device) {
    return {
      disabled: true,
      reason: adapter === "a30" || adapter === "elm327" ? "Select a paired Bluetooth device" : "Select an OBDLink SX port",
    };
  }
  if (blocked) return { disabled: true, reason: "Stop acquisition, recording, or replay first" };
  const identifierWidth = candidate.service === "21" ? 2 : candidate.service === "22" ? 4 : 0;
  const validHex = (value: string, width: number) => value.length === width && /^[0-9a-f]+$/i.test(value);
  const validCanHeader = (value: string) => validHex(value, 3) && Number.parseInt(value, 16) <= 0x7ff;
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
