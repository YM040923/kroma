import type { Library } from '@kromatv/client/media';
import { libraryPreferenceId } from '#web/features/catalog/library-identity';
import type { MessageKey, Translate } from '@kromatv/i18n';
import type { HomeSection } from './entries';

export interface HomeLayout {
  order: string[];
  hidden: string[];
}

export const DEFAULT_HOME_LAYOUT: HomeLayout = { order: [], hidden: [] };
export const HERO_SLOT = 'hero';
export const CONTINUE_SLOT = 'continue';

const permanent: ReadonlyArray<readonly [string, MessageKey]> = [
  [HERO_SLOT, 'account.homeHero'], [CONTINUE_SLOT, 'content.continueWatching'],
  ['new-episodes', 'content.newEpisodes'], ['for-you', 'content.forYou'], ['trending', 'content.trending'],
  ['spotlight', 'content.spotlight'], ['recent', 'content.recentMovies'], ['recent-shows', 'content.recentShows'],
  ['my-list', 'nav.myList'], ['acclaimed', 'content.acclaimed'], ['quick', 'content.quickWatch'], ['watch-again', 'content.watchAgain'],
];

export function slotId(section: HomeSection): string {
  if (section.layout === 'hero') return HERO_SLOT;
  if (/^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$/.test(section.id)) return section.id;
  // Person and curator keys may contain non-Latin names; keep their personal switches independent.
  let code = 0xcbf29ce484222325n;
  for (const c of section.id) code = BigInt.asUintN(64, (code ^ BigInt(c.codePointAt(0)!)) * 0x100000001b3n);
  return `row-${code.toString(36)}`;
}

export function orderedModules<T extends { id: string }>(modules: readonly T[], order: readonly string[]): T[] {
  const remaining = new Map(modules.map((row) => [row.id, row]));
  const first: T[] = [];
  for (const id of order) {
    const row = remaining.get(id);
    if (row) {
      first.push(row);
      remaining.delete(id);
    }
  }
  return first.concat([...remaining.values()]);
}

export function moveModule(order: readonly string[], id: string, offset: -1 | 1): string[] {
  const next = [...order];
  const previous = next.indexOf(id);
  const target = previous + offset;
  if (previous >= 0 && target >= 0 && target < next.length) {
    next.splice(previous, 1);
    next.splice(target, 0, id);
  }
  return next;
}

type HomeSlot = { id: string; section?: HomeSection; library?: Library };

export function homeSlots(sections: readonly HomeSection[], libraries: readonly Library[] = [], includeEmptyLibraries = false): HomeSlot[] {
  const hero = sections.find((s) => s.layout === 'hero' && s.items.length > 0);
  return [
    { id: HERO_SLOT, section: hero }, { id: CONTINUE_SLOT },
    ...libraries.filter((lib) => includeEmptyLibraries || lib.itemCount > 0).map((library) => ({ id: libraryPreferenceId(library.id), library })),
    ...sections.filter((s) => s.layout !== 'hero' && s.items.length > 0).map((section) => ({ id: slotId(section), section })),
  ];
}

export function layoutInventory(sections: readonly HomeSection[], t: Translate, hasContinue: boolean, libraries: readonly Library[] = []) {
  const labels = new Map(permanent);
  const rows = homeSlots(sections, libraries, true).map(({ id, section, library }) => ({
    id,
    title: library?.name ?? (id === HERO_SLOT ? t('account.homeHero') : section?.title ?? t('content.continueWatching')),
    available: library ? library.itemCount > 0 : id === HERO_SLOT || (id === CONTINUE_SLOT ? hasContinue : !!section),
  }));
  for (const { id } of rows) labels.delete(id);
  return [...rows, ...Array.from(labels, ([id, key]) => ({ id, title: t(key), available: false }))];
}
