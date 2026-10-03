export type BrowserRecording = {
  path: string;
  name: string;
  modified_unix_ms: number;
  sample_count: number;
};

export type BrowserRecordingRow = readonly [
  path: string,
  name: string,
  modified_unix_ms: number,
  sample_count: number,
];

export function recentRecordingsFirst(recordings: readonly BrowserRecording[]): BrowserRecording[] {
  return [...recordings].sort((left, right) =>
    right.modified_unix_ms - left.modified_unix_ms || left.name.localeCompare(right.name));
}

export async function openAndActivateRecording<T>(
  load: () => Promise<T>,
  activate: (recording: T) => void | Promise<void>,
): Promise<void> {
  await activate(await load());
}

export function formatRecordingTime(unixMs: number): string {
  const date = new Date(unixMs);
  return Number.isNaN(date.getTime())
    ? "Unknown time"
    : new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(date);
}
