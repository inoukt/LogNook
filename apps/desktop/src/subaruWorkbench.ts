/** A local catalog selection prepares an operator-controlled test; it never transmits. */
export interface SubaruWorkbenchCandidate {
  id: string;
  name: string;
  protocol: string;
  request_header: string;
  response_header: string;
  request_hint: string;
  can_manual_read: boolean;
  manual_read_reason: string;
}

export interface SubaruWorkbenchSelection {
  token: number;
  id: string;
  name: string;
  protocol: string;
  requestId: string;
  responseId: string;
  extended: boolean;
  kind: "mode_21" | "mode_22";
  identifier: string;
}

export function prepareSubaruWorkbench(
  row: SubaruWorkbenchCandidate,
  token: number,
): SubaruWorkbenchSelection | null {
  if (!row.can_manual_read) return null;
  if (row.protocol.trim().toUpperCase() !== "FHI-CAN") return null;
  const request = parseHeader(row.request_header);
  const response = parseHeader(row.response_header);
  const hint = row.request_hint.trim().toUpperCase().match(/^(21|22)\s+([0-9A-F]{2}|[0-9A-F]{4})$/);
  if (request === null || response === null || request.value === response.value || !hint) return null;
  if (request.extended !== response.extended) return null;
  if (hint[2].length !== (hint[1] === "21" ? 2 : 4)) return null;
  return {
    token, id: row.id, name: row.name, protocol: row.protocol,
    requestId: request.value.toString(16).toUpperCase(),
    responseId: response.value.toString(16).toUpperCase(), extended: request.extended,
    kind: hint[1] === "21" ? "mode_21" : "mode_22", identifier: hint[2],
  };
}

function parseHeader(text: string): { value: number; extended: boolean } | null {
  const normalized = text.trim().replace(/^(?:\$|0x)/i, "");
  if (!/^[0-9a-f]{1,8}$/i.test(normalized)) return null;
  const value = Number.parseInt(normalized, 16);
  return value <= 0x1fffffff ? { value, extended: normalized.length > 3 || value > 0x7ff } : null;
}
