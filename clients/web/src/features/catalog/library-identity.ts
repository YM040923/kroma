import type { Library } from '@kromatv/client/media';
import type { IconName } from '@kromatv/ui/kit';

export function navigationPreferenceId(prefix: string, id: string): string {
  const readable = `${prefix}:${id}`;
  if (readable.length <= 64 && /^[A-Za-z0-9._:-]+$/.test(id)) return readable;
  let hash = 0xcbf29ce484222325n;
  for (const c of id) hash = BigInt.asUintN(64, (hash ^ BigInt(c.codePointAt(0)!)) * 0x100000001b3n);
  return `${prefix}.hash:${hash.toString(36)}`;
}

export const libraryPreferenceId = (id: string) => navigationPreferenceId('lib', id);

export const LIBRARY_ICONS = {
  movies: 'movie', shows: 'device-tv', mixed: 'library', photos: 'photo', other: 'folder',
} as const satisfies Record<Library['kind'], IconName>;
