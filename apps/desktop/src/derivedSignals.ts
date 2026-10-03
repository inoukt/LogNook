import type { SignalOption } from "./workspace";

export type DerivedSignalDefinition = {
  id: string;
  name: string;
  expression: string;
  unit: string;
  tare_offset: number;
};

export type DerivedSignalSource = { id: string; name: string; unit: string };

export function newDerivedSignalDefinition(
  definitions: readonly DerivedSignalDefinition[],
  source?: DerivedSignalSource | null,
): DerivedSignalDefinition {
  let number = definitions.length + 1;
  while (definitions.some(({ id }) => id === `derived_${number}`)) number++;
  return {
    id: `derived_${number}`,
    name: source ? `${source.name} derived` : `Derived signal ${number}`,
    expression: source ? `${source.id} * 1` : "engine_rpm * 1",
    unit: source?.unit ?? "rpm",
    tare_offset: 0,
  };
}

export function isAvailableDerivedSignalId(
  candidate: string,
  editingId: string,
  definitions: readonly DerivedSignalDefinition[],
  baseSignalIds: readonly string[],
): boolean {
  return /^[A-Za-z_][A-Za-z0-9_]*$/.test(candidate)
    && !baseSignalIds.includes(candidate)
    && !definitions.some(({ id }) => id !== editingId && id === candidate);
}

export function restoreDerivedSignalDefinitions(
  serialized: string | null,
  baseSignalIds: string[],
): DerivedSignalDefinition[] {
  let saved: unknown;
  try { saved = JSON.parse(serialized ?? "null"); } catch { return []; }
  if (!Array.isArray(saved)) return [];
  const baseIds = new Set(baseSignalIds);
  const seen = new Set<string>();
  const definitions: DerivedSignalDefinition[] = [];
  for (const value of saved) {
    if (!value || typeof value !== "object" || Array.isArray(value)) continue;
    const item = value as Partial<DerivedSignalDefinition>;
    if (typeof item.id !== "string" || !/^[A-Za-z_][A-Za-z0-9_]*$/.test(item.id)) continue;
    if (baseIds.has(item.id) || seen.has(item.id)) continue;
    if (typeof item.name !== "string" || !item.name.trim() || item.name.length > 100) continue;
    if (typeof item.expression !== "string" || !item.expression.trim() || item.expression.length > 512) continue;
    if (typeof item.unit !== "string" || !item.unit.trim() || item.unit.length > 32) continue;
    if (typeof item.tare_offset !== "number" || !Number.isFinite(item.tare_offset)) continue;
    seen.add(item.id);
    definitions.push({
      id: item.id,
      name: item.name.trim(),
      expression: item.expression.trim(),
      unit: item.unit.trim(),
      tare_offset: item.tare_offset,
    });
    if (definitions.length === 32) break;
  }
  return definitions;
}

export function derivedSignalOptions(definitions: DerivedSignalDefinition[]): SignalOption[] {
  return definitions.map(({ id, name, expression, unit }) => ({
    id,
    name,
    unit,
    description: `Derived from ${expression}`,
  }));
}
