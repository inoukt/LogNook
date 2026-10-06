export type AdapterKind = "simulator" | "obdlink_sx" | "openport2" | "elm327" | "a30";

export type ConnectionControlInput = {
  busy: boolean;
  recording: boolean;
  replaying: boolean;
  hasSignals: boolean;
  sxPort: string;
  sxIdentityMatches: boolean;
  openPortProvider: string;
  openPortIdentityMatches: boolean;
  bluetoothAddress: string;
  bluetoothIdentityMatches: boolean;
};

export function adapterLabel(adapter: AdapterKind): string {
  if (adapter === "obdlink_sx") return "OBDLink SX";
  if (adapter === "openport2") return "OpenPort 2";
  if (adapter === "elm327") return "Bluetooth ELM327";
  if (adapter === "a30") return "Xtool A30/A30M";
  return "Simulator";
}

export function connectionControlState(adapter: AdapterKind, state: ConnectionControlInput) {
  const ready = !state.busy && !state.recording && !state.replaying;
  const hasAdapter = adapter === "simulator"
    || (adapter === "obdlink_sx" ? Boolean(state.sxPort)
      : adapter === "openport2" ? Boolean(state.openPortProvider)
        : Boolean(state.bluetoothAddress));
  const identified = adapter === "simulator"
    || (adapter === "obdlink_sx" ? state.sxIdentityMatches
      : adapter === "openport2" ? state.openPortIdentityMatches
        : state.bluetoothIdentityMatches);
  return {
    identifyDisabled: !ready || adapter === "simulator" || !hasAdapter,
    connectDisabled: !ready || !state.hasSignals || !hasAdapter || !identified,
    connectLabel: `Connect ${adapterLabel(adapter)}`,
  };
}
