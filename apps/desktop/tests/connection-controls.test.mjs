import assert from "node:assert/strict";
import test from "node:test";

const controls = await import("../src/connectionControls.ts").catch(() => null);

test("profile standby connects without a preselected polling set while preserving other guards", () => {
  const state = {
    busy: false, recording: false, replaying: false, hasSignals: false,
    sxPort: "COM8", sxIdentityMatches: true, openPortProvider: "", openPortIdentityMatches: false,
    bluetoothAddress: "", bluetoothIdentityMatches: false,
  };
  assert.equal(controls.connectionControlState("obdlink_sx", state).connectDisabled, true);
  assert.equal(controls.connectionControlState("obdlink_sx", { ...state, profileStandby: true }).connectDisabled, false);
  assert.equal(controls.connectionControlState("obdlink_sx", { ...state, profileStandby: true, recording: true }).connectDisabled, true);
  assert.equal(controls.connectionControlState("obdlink_sx", { ...state, profileStandby: true, sxIdentityMatches: false }).connectDisabled, true);
});

test("adapter labels stay short for the one-row toolbar", () => {
  assert.ok(controls, "connection controls helper is available");
  assert.equal(controls.adapterLabel("simulator"), "Simulator");
  assert.equal(controls.adapterLabel("obdlink_sx"), "OBDLink SX");
  assert.equal(controls.adapterLabel("openport2"), "OpenPort 2");
  assert.equal(controls.adapterLabel("elm327"), "Bluetooth ELM327");
  assert.equal(controls.adapterLabel("a30"), "Xtool A30/A30M");
  assert.equal(controls.adapterLabel("candlelight_obd"), "Candlelight OBD (GS-USB)");
});

test("Candlelight OBD needs a discovered USB device, matching Identify, and selected signals", () => {
  assert.ok(controls, "connection controls helper is available");
  const state = {
    busy: false,
    recording: false,
    replaying: false,
    hasSignals: true,
    sxPort: "",
    sxIdentityMatches: false,
    openPortProvider: "",
    openPortIdentityMatches: false,
    bluetoothAddress: "",
    bluetoothIdentityMatches: false,
    canObdDevice: "",
    canObdIdentityMatches: false,
  };
  assert.equal(controls.connectionControlState("candlelight_obd", state).identifyDisabled, true);
  assert.equal(controls.connectionControlState("candlelight_obd", state).connectDisabled, true);

  const selected = { ...state, canObdDevice: "1:4" };
  assert.equal(controls.connectionControlState("candlelight_obd", selected).identifyDisabled, false);
  assert.equal(controls.connectionControlState("candlelight_obd", selected).connectDisabled, true);

  const identified = { ...selected, canObdIdentityMatches: true };
  assert.equal(controls.connectionControlState("candlelight_obd", identified).connectDisabled, false);
  assert.equal(controls.connectionControlState("candlelight_obd", { ...identified, hasSignals: false }).connectDisabled, true);
  assert.equal(controls.connectionControlState("candlelight_obd", { ...identified, profileStandby: true, hasSignals: false }).connectDisabled, true);
});

test("Bluetooth adapters need a paired address and matching identity before connect", () => {
  assert.ok(controls, "connection controls helper is available");
  for (const adapter of ["elm327", "a30"]) {
    const missingDevice = controls.connectionControlState(adapter, {
      busy: false,
      recording: false,
      replaying: false,
      hasSignals: true,
      sxPort: "",
      sxIdentityMatches: false,
      openPortProvider: "",
      openPortIdentityMatches: false,
      bluetoothAddress: "",
      bluetoothIdentityMatches: false,
    });
    assert.equal(missingDevice.identifyDisabled, true);
    assert.equal(missingDevice.connectDisabled, true);

    const paired = controls.connectionControlState(adapter, {
      busy: false,
      recording: false,
      replaying: false,
      hasSignals: true,
      sxPort: "",
      sxIdentityMatches: false,
      openPortProvider: "",
      openPortIdentityMatches: false,
      bluetoothAddress: "AA:BB:CC:DD:EE:FF",
      bluetoothIdentityMatches: false,
    });
    assert.equal(paired.identifyDisabled, false);
    assert.equal(paired.connectDisabled, true);

    const identified = controls.connectionControlState(adapter, {
      busy: false,
      recording: false,
      replaying: false,
      hasSignals: true,
      sxPort: "",
      sxIdentityMatches: false,
      openPortProvider: "",
      openPortIdentityMatches: false,
      bluetoothAddress: "AA:BB:CC:DD:EE:FF",
      bluetoothIdentityMatches: true,
    });
    assert.equal(identified.connectDisabled, false);
  }
});

test("identify and connect stay disabled until an SX port and identity are selected", () => {
  assert.ok(controls, "connection controls helper is available");
  const state = controls.connectionControlState("obdlink_sx", {
    busy: false,
    recording: false,
    replaying: false,
    hasSignals: true,
    sxPort: "",
    sxIdentityMatches: false,
    openPortProvider: "",
    openPortIdentityMatches: false,
  });
  assert.equal(state.identifyDisabled, true);
  assert.equal(state.connectDisabled, true);

  const portSelected = controls.connectionControlState("obdlink_sx", {
    busy: false,
    recording: false,
    replaying: false,
    hasSignals: true,
    sxPort: "COM5",
    sxIdentityMatches: false,
    openPortProvider: "",
    openPortIdentityMatches: false,
  });
  assert.equal(portSelected.identifyDisabled, false);
  assert.equal(portSelected.connectDisabled, true);
});

test("OpenPort identity and connect require a selected provider and matching identity", () => {
  assert.ok(controls, "connection controls helper is available");
  const missingProvider = controls.connectionControlState("openport2", {
    busy: false,
    recording: false,
    replaying: false,
    hasSignals: true,
    sxPort: "",
    sxIdentityMatches: false,
    openPortProvider: "",
    openPortIdentityMatches: false,
  });
  assert.equal(missingProvider.identifyDisabled, true);
  assert.equal(missingProvider.connectDisabled, true);

  const providerSelected = controls.connectionControlState("openport2", {
    busy: false,
    recording: false,
    replaying: false,
    hasSignals: true,
    sxPort: "",
    sxIdentityMatches: false,
    openPortProvider: "opta",
    openPortIdentityMatches: false,
  });
  assert.equal(providerSelected.identifyDisabled, false);
  assert.equal(providerSelected.connectDisabled, true);

  const identified = controls.connectionControlState("openport2", {
    busy: false,
    recording: false,
    replaying: false,
    hasSignals: true,
    sxPort: "",
    sxIdentityMatches: false,
    openPortProvider: "opta",
    openPortIdentityMatches: true,
  });
  assert.equal(identified.connectDisabled, false);
});

test("OpenPort Subaru SSM mode can connect without standard OBD signal selections", () => {
  assert.ok(controls, "connection controls helper is available");
  const state = {
    busy: false,
    recording: false,
    replaying: false,
    hasSignals: false,
    sxPort: "",
    sxIdentityMatches: false,
    openPortProvider: "opta",
    openPortIdentityMatches: true,
    openPortMode: "obd_can",
    bluetoothAddress: "",
    bluetoothIdentityMatches: false,
  };
  assert.equal(controls.connectionControlState("openport2", state).connectDisabled, true);
  const ssm = controls.connectionControlState("openport2", { ...state, openPortMode: "subaru_ssm" });
  assert.equal(ssm.connectDisabled, false);
  assert.equal(ssm.connectLabel, "Connect Subaru SSM");
  assert.equal(
    controls.connectionControlState("openport2", { ...state, openPortMode: "subaru_ssm", recording: true }).connectDisabled,
    true,
  );
});

test("simulator connect does not depend on hardware identity", () => {
  assert.ok(controls, "connection controls helper is available");
  const state = controls.connectionControlState("simulator", {
    busy: false,
    recording: false,
    replaying: false,
    hasSignals: true,
    sxPort: "",
    sxIdentityMatches: false,
    openPortProvider: "",
    openPortIdentityMatches: false,
  });
  assert.equal(state.identifyDisabled, true);
  assert.equal(state.connectDisabled, false);
});
