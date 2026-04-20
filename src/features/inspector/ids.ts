// Atlas - client-side id helpers for Inspector mutations.

export function newId(): string {
  // Vite's modern target + Tauri's WebView always provide `crypto.randomUUID`.
  return crypto.randomUUID();
}

export function slugify(input: string): string {
  return input
    .toLowerCase()
    .trim()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

// `slugify(name)` plus a short random suffix - collision-resistant when two
export function newScriptId(name: string): string {
  const slug = slugify(name);
  if (!slug) return newId();
  // 4-char base36 suffix is enough: 1.7M combos, enough to dedupe within a
  const suffix = Math.random().toString(36).slice(2, 6);
  return `${slug}-${suffix}`;
}
