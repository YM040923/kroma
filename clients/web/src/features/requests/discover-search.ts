// The `/search` url: the text query, the source tab, and every discovery
// filter. Everything lives here so the page can be shared, refreshed and
// navigated back into with its filters intact. Filters the active source does
// not use are still validated (a url may carry both tabs' picks), and the page
// only reads the ones its source owns.

import type { DiscoverType } from '@kromatv/client/discovery';

/** Where results come from. Douban is deliberately absent: there is no data
 * source behind it yet, so the UI offers it as a disabled placeholder only. */
export type DiscoverSource = 'library' | 'tmdb';

/** TMDB discover ordering. */
export type DiscoverSort = 'popularity' | 'release' | 'rating';

/** Local catalogue watched-state filter. */
export type WatchedFilter = 'any' | 'watched' | 'unwatched';

/** Whether a TMDB result is already in this server's catalogue. */
export type AvailabilityFilter = 'any' | 'in' | 'out';

/** Links may name only q/type; the validator fills the rest at runtime. */
export type DiscoverSearchLink = Partial<DiscoverSearch> & Pick<DiscoverSearch, 'q' | 'type'>;

export interface DiscoverSearch {
  q: string;
  type: DiscoverType;
  source: DiscoverSource;
  page: number;
  /** Shared: an exact release/first-air year ... */
  year?: number;
  /** ... or the decade it belongs to (`1990` = the 1990s). */
  decade?: number;
  /** TMDB genre id; local metadata stores the same ids. */
  genre?: number;
  /** TMDB only. */
  sort: DiscoverSort;
  lang?: string;
  state: AvailabilityFilter;
  /** Local only. */
  library?: string;
  rating?: number;
  watched: WatchedFilter;
}

export const DEFAULT_DISCOVER_SEARCH: DiscoverSearch = {
  q: '',
  type: 'all',
  source: 'tmdb',
  page: 1,
  sort: 'popularity',
  state: 'any',
  watched: 'any',
};

const MAX_QUERY_CHARS = 128;
const MAX_LIBRARY_CHARS = 64;

function text(value: unknown, max: number): string | undefined {
  return typeof value === 'string' && value.trim() !== '' ? value.slice(0, max) : undefined;
}

function integer(value: unknown, min: number, max: number): number | undefined {
  const n = typeof value === 'string' && value.trim() !== '' ? Number(value) : value;
  return typeof n === 'number' && Number.isInteger(n) && n >= min && n <= max ? n : undefined;
}

function decimal(value: unknown, min: number, max: number): number | undefined {
  const n = typeof value === 'string' && value.trim() !== '' ? Number(value) : value;
  return typeof n === 'number' && Number.isFinite(n) && n >= min && n <= max ? n : undefined;
}

function oneOf<T extends string>(value: unknown, allowed: readonly T[]): T | undefined {
  return typeof value === 'string' && (allowed as readonly string[]).includes(value)
    ? (value as T)
    : undefined;
}

const TYPES: readonly DiscoverType[] = ['all', 'movie', 'tv'];
const SOURCES: readonly DiscoverSource[] = ['library', 'tmdb'];
const SORTS: readonly DiscoverSort[] = ['popularity', 'release', 'rating'];
const STATES: readonly AvailabilityFilter[] = ['any', 'in', 'out'];
const WATCHED: readonly WatchedFilter[] = ['any', 'watched', 'unwatched'];
const LANGUAGE = /^[a-z]{2,3}$/;

/** Validate a `/search` url. Unknown values fall back to the default for that
 * key; a filter that is not set is absent (not `undefined`) so it does not
 * round-trip into the url as an empty parameter. */
export function validateDiscoverSearch(s: Record<string, unknown>): DiscoverSearchLink {
  const decade = integer(s.decade, 1870, 2100);
  const out: DiscoverSearch = {
    q: text(s.q, MAX_QUERY_CHARS) ?? '',
    type: oneOf(s.type, TYPES) ?? DEFAULT_DISCOVER_SEARCH.type,
    source: oneOf(s.source, SOURCES) ?? DEFAULT_DISCOVER_SEARCH.source,
    page: integer(s.page, 1, 10_000) ?? DEFAULT_DISCOVER_SEARCH.page,
    sort: oneOf(s.sort, SORTS) ?? DEFAULT_DISCOVER_SEARCH.sort,
    state: oneOf(s.state, STATES) ?? DEFAULT_DISCOVER_SEARCH.state,
    watched: oneOf(s.watched, WATCHED) ?? DEFAULT_DISCOVER_SEARCH.watched,
  };
  const year = integer(s.year, 1870, 2100);
  if (year !== undefined) {
    out.year = year;
  }
  if (decade !== undefined && decade % 10 === 0) {
    out.decade = decade;
  }
  const genre = integer(s.genre, 1, 200_000);
  if (genre !== undefined) {
    out.genre = genre;
  }
  const lang = typeof s.lang === 'string' ? s.lang.toLowerCase() : '';
  if (LANGUAGE.test(lang)) {
    out.lang = lang;
  }
  const library = text(s.library, MAX_LIBRARY_CHARS);
  if (library !== undefined) {
    out.library = library;
  }
  const rating = decimal(s.rating, 0, 10);
  if (rating !== undefined) {
    out.rating = rating;
  }
  return out;
}

/** Every filter set away from its default, across both tabs. Drives the
 * "clear all" affordance and its label. */
export function activeFilterCount(search: Partial<DiscoverSearch>): number {
  let n = 0;
  for (const key of [
    'year',
    'decade',
    'genre',
    'lang',
    'library',
    'rating',
  ] as const) {
    if (search[key] !== undefined) {
      n += 1;
    }
  }
  if (search.sort !== DEFAULT_DISCOVER_SEARCH.sort) {
    n += 1;
  }
  if (search.state !== DEFAULT_DISCOVER_SEARCH.state) {
    n += 1;
  }
  if (search.watched !== DEFAULT_DISCOVER_SEARCH.watched) {
    n += 1;
  }
  return n;
}

/** What "clear all" writes: every filter back to its default, the query and
 * the source tab kept so clearing does not throw the reader out of the tab. */
export function clearedDiscoverSearch(search: DiscoverSearchLink): DiscoverSearch {
  return {
    ...DEFAULT_DISCOVER_SEARCH,
    q: search.q,
    type: search.type,
    source: search.source ?? DEFAULT_DISCOVER_SEARCH.source,
  };
}
