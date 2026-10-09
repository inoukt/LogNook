import { writable } from "svelte/store";

const canHelpPreferenceKey = "lognook-can-help-bubbles-v1";

let initiallyEnabled = true;
try {
  initiallyEnabled = typeof window === "undefined"
    || window.localStorage.getItem(canHelpPreferenceKey) !== "false";
} catch {
  // Browser storage is optional; keep the default enabled for this run.
}

export const canHelpBubblesEnabled = writable(initiallyEnabled);

export function setCanHelpBubblesEnabled(enabled: boolean) {
  canHelpBubblesEnabled.set(enabled);
  try {
    if (typeof window !== "undefined") window.localStorage.setItem(canHelpPreferenceKey, String(enabled));
  } catch {
    // Keep the in-memory preference even when browser storage is unavailable.
  }
}
