/** Presentation data for the selected catalog row; actions use existing owners. */
export type CatalogInspection = {
  id: string;
  name: string;
  subtitle: string;
  status: string;
  tone?: "neutral" | "value" | "warning";
  value?: string;
  note?: string;
  fields: { label: string; value: string }[];
  blocks?: { label: string; value: string }[];
  links?: { label: string; url: string }[];
  actions: { label: string; run: () => void; disabled?: boolean; title?: string }[];
};
