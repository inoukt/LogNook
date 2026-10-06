export function toggleCandidateSelection(
  current: readonly string[],
  candidateId: string,
  selected: boolean,
): string[] {
  const next = [...new Set(current)];
  if (!selected) return next.filter((id) => id !== candidateId);
  if (!next.includes(candidateId)) next.push(candidateId);
  return next;
}

export async function runResearchQueue<T>(
  items: readonly string[],
  runOne: (id: string, index: number) => Promise<T>,
  shouldStop: () => boolean,
  onItem: (id: string, index: number, result: T | Error) => void,
): Promise<void> {
  for (const [index, id] of items.entries()) {
    if (shouldStop()) return;

    let result: T;
    try {
      result = await runOne(id, index);
    } catch (error) {
      onItem(id, index, error instanceof Error ? error : new Error(String(error)));
      return;
    }
    onItem(id, index, result);
  }
}
