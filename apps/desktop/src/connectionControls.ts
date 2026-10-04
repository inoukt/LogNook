export type AdapterKind = "simulator" | "obdlink_sx" | "openport2";

export type ConnectionControlInput = {
  busy: boolean;
  recording: boolean;
  replaying: boolean;
  hasSignals: boolean;
  sxPort: string;
  sxIdentityMatches: boolean;
  openPortProvider: string;
  openPortIdentityMatches: boolean;
};

export function adapterLabel(adapter: AdapterKind): string {
  if (adapter === "obdlink_sx") return "OBDLink SX";
  if (adapter === "openport2") return "OpenPort 2";
  return "Simulator";
}

export function connectionControlState(adapter: AdapterKind, state: ConnectionControlInput) {
  const ready = !state.busy && !state.recording && !state.replaying;
  const hasAdapter = adapter === "simulator"
    || (adapter === "obdlink_sx" ? Boolean(state.sxPort) : Boolean(state.openPortProvider));
  const identified = adapter === "simulator"
    || (adapter === "obdlink_sx" ? state.sxIdentityMatches : state.openPortIdentityMatches);
  return {
    identifyDisabled: !ready || adapter === "simulator" || !hasAdapter,
    connectDisabled: !ready || !state.hasSignals || !hasAdapter || !identified,
    connectLabel: `Connect ${adapterLabel(adapter)}`,
  };
}
