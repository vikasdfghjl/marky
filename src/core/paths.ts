// Windows path and URL helpers shared by the page and the preview worker.

export const UNTITLED = 'Untitled';

/** `scheme:` at the start of a link or image source (http:, file:, javascript:, …). */
export const URL_SCHEME = /^[a-z][a-z0-9+.-]*:/i;

/** Exact spelling identifies paths because Windows directories may be case-sensitive. */
export function samePathSpelling(a: string, b: string): boolean {
  return a === b;
}

/** Last path segment, or "Untitled" for documents that were never saved. */
export function fileName(path: string | null): string {
  if (!path) return UNTITLED;
  return path.split(/[\\/]/).pop()!;
}

/** Containing folder, or null for documents that were never saved. */
export function folderOf(path: string | null): string | null {
  if (!path) return null;
  return path.replace(/[\\/][^\\/]*$/, '');
}

/** Joins and normalises `.`/`..` segments; never climbs above the drive or share root. */
export function joinWindowsPath(folder: string, relative: string): string {
  const parts = folder.split(/[\\/]+/).filter(Boolean);
  const rootLength = folder.startsWith('\\\\') ? 2 : 1; // \\server\share or C:
  for (const segment of relative.split(/[\\/]+/)) {
    if (segment === '' || segment === '.') continue;
    if (segment === '..') {
      if (parts.length > rootLength) parts.pop();
      continue;
    }
    parts.push(segment);
  }
  const prefix = folder.startsWith('\\\\') ? '\\\\' : '';
  return prefix + parts.join('\\');
}
