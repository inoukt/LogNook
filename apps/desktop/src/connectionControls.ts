export type AdapterKind = "simulator" | "obdlink_sx" | "openport2" | "elm327" | "a30" | "candlelight_obd";

export type ConnectionControlInput = {
  busy: boolean;
  recording: boolean;
  replaying: boolean;
  hasSignals: boolean;
  profileStandby?: boolean;
  sxPort: string;
  sxIdentityMatches: boolean;
  openPortProvider: string;
  openPortIdentityMatches: boolean;
  openPortMode?: "obd_can" | "subaru_ssm";
  bluetoothAddress: string;
  bluetoothIdentityMatches: boolean;
  canObdDevice?: string;
  canObdIdentityMatches?: boolean;
};

export function adapterLabel(adapter: AdapterKind): string {
  if (adapter === "obdlink_sx") return "OBDLink SX";
  if (adapter === "openport2") return "OpenPort 2";
  if (adapter === "elm327") return "Bluetooth ELM327";
  if (adapter === "a30") return "Xtool A30/A30M";
  if (adapter === "candlelight_obd") return "Candlelight OBD (GS-USB)";
  return "Simulator";
}

export function connectionControlState(adapter: AdapterKind, state: ConnectionControlInput) {
  const ready = !state.busy && !state.recording && !state.replaying;
  const hasAdapter = adapter === "simulator"
    || (adapter === "obdlink_sx" ? Boolean(state.sxPort)
      : adapter === "openport2" ? Boolean(state.openPortProvider)
        : adapter === "candlelight_obd" ? Boolean(state.canObdDevice)
          : Boolean(state.bluetoothAddress));
  const identified = adapter === "simulator"
    || (adapter === "obdlink_sx" ? state.sxIdentityMatches
      : adapter === "openport2" ? state.openPortIdentityMatches
        : adapter === "candlelight_obd" ? state.canObdIdentityMatches
          : state.bluetoothIdentityMatches);
  const profileStandby = adapter !== "candlelight_obd" && state.profileStandby;
  const hasSignals = state.hasSignals || (adapter === "openport2" && state.openPortMode === "subaru_ssm");
  return {
    identifyDisabled: !ready || adapter === "simulator" || !hasAdapter,
    connectDisabled: !ready || (!hasSignals && !profileStandby) || !hasAdapter || !identified,
    connectLabel: adapter === "openport2" && state.openPortMode === "subaru_ssm"
      ? "Connect Subaru SSM"
      : `Connect ${adapterLabel(adapter)}`,
  };
}
