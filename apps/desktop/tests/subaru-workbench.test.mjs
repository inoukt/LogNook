import assert from "node:assert/strict";
import test from "node:test";
import { prepareSubaruWorkbench } from "../src/subaruWorkbench.ts";

const row = {
  id: "subaru:test", name: "Test signal", protocol: "FHI-CAN",
  request_header: "$7E0", response_header: "$7E8", request_hint: "22 1234",
  can_manual_read: true, manual_read_reason: "Inferred mapping",
};

test("Subaru selection prepares one exact identifier without starting a request", () => {
  const selected = prepareSubaruWorkbench(row, 7);
  assert.deepEqual(selected, { token: 7, id: row.id, name: row.name, protocol: row.protocol,
    requestId: "7E0", responseId: "7E8", extended: false, kind: "mode_22", identifier: "1234" });
  assert.equal(row.request_header, "$7E0");
});

test("Subaru prefill preserves Mode 21 width and extended addressing", () => {
  const selected = prepareSubaruWorkbench({ ...row, request_header: "0x18DA10F1",
    response_header: "18DAF110", request_hint: "21 0A" }, 8);
  assert.equal(selected.kind, "mode_21");
  assert.equal(selected.identifier, "0A");
  assert.equal(selected.extended, true);
});

test("Subaru prefill rejects unsupported, invalid and vehicle-changing request forms", () => {
  assert.equal(prepareSubaruWorkbench({ ...row, can_manual_read: false }, 1), null);
  for (const protocol of ["Phase4-CAN", "Phase5-CAN", "SSM-KWP", "KWP14230"]) {
    assert.equal(prepareSubaruWorkbench({ ...row, protocol }, 1), null);
  }
  for (const request_hint of ["10 03", "27 01", "2E 1234", "21 1234", "22 12", "22 1234 00"]) {
    assert.equal(prepareSubaruWorkbench({ ...row, request_hint }, 1), null);
  }
  for (const response_header of ["", "n/a", "FFFFFFFF", "$7E0"]) {
    assert.equal(prepareSubaruWorkbench({ ...row, response_header }, 1), null);
  }
});

test("Subaru prefill keeps explicit extended CAN format and rejects mixed routes", () => {
  assert.equal(prepareSubaruWorkbench({ ...row, request_header: "000007E0", response_header: "000007E8" }, 1).extended, true);
  assert.equal(prepareSubaruWorkbench({ ...row, response_header: "18DAF110" }, 1), null);
});
