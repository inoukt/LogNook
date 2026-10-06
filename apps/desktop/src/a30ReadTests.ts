export const A30_READ_TESTS = [
  ["mode_01_engine_rpm", "Mode 01 · RPM"],
  ["mode_02_freeze_frame_rpm", "Mode 02 · freeze frame RPM"],
  ["mode_03_stored_dtcs", "Mode 03 · stored DTCs"],
  ["mode_06_bitmap_00", "Mode 06 · bitmap 00"],
  ["mode_06_bitmap_20", "Mode 06 · bitmap 20"],
  ["mode_06_bitmap_40", "Mode 06 · bitmap 40"],
  ["mode_06_bitmap_60", "Mode 06 · bitmap 60"],
  ["mode_06_bitmap_80", "Mode 06 · bitmap 80"],
  ["mode_06_bitmap_a0", "Mode 06 · bitmap A0"],
  ["mode_06_tid_35", "Mode 06 · Scion TID 35"],
  ["mode_06_tid_a2", "Mode 06 · Scion TID A2"],
  ["mode_06_tid_a5", "Mode 06 · Scion TID A5"],
  ["mode_07_pending_dtcs", "Mode 07 · pending DTCs"],
  ["mode_09_vin", "Mode 09 · VIN"],
  ["mode_0a_permanent_dtcs", "Mode 0A · permanent DTCs"],
] as const;

export type A30ReadTestId = (typeof A30_READ_TESTS)[number][0];
