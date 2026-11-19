import { describe, expect, it } from 'vitest';
import {
  activeFilterCount,
  clearedDiscoverSearch,
  DEFAULT_DISCOVER_SEARCH,
  validateDiscoverSearch,
} from './discover-search';

const defaults = (patch: Record<string, unknown> = {}) => ({
  ...DEFAULT_DISCOVER_SEARCH,
  ...patch,
});

describe('validateDiscoverSearch', () => {
  it('defaults to an empty query, all types, the TMDB tab and no filters', () => {
    expect(validateDiscoverSearch({})).toEqual(DEFAULT_DISCOVER_SEARCH);
  });

  it('keeps a query string, capped at the server bound', () => {
    expect(validateDiscoverSearch({ q: 'the matrix' }).q).toBe('the matrix');
    expect(validateDiscoverSearch({ q: 'a'.repeat(200) }).q).toHaveLength(128);
  });

  it('keeps a recognised type and falls back otherwise', () => {
    expect(validateDiscoverSearch({ type: 'movie' }).type).toBe('movie');
    expect(validateDiscoverSearch({ type: 'tv' }).type).toBe('tv');
    expect(validateDiscoverSearch({ type: 'sideways' }).type).toBe('all');
  });

  it('keeps the library and TMDB source tabs', () => {
    expect(validateDiscoverSearch({ source: 'library' }).source).toBe('library');
    expect(validateDiscoverSearch({ source: 'douban' }).source).toBe('tmdb');
  });

  it('keeps a positive page and drops a nonsense one', () => {
    expect(validateDiscoverSearch({ page: '3' }).page).toBe(3);
    for (const page of [0, -2, 1.5, 'x', null]) {
      expect(validateDiscoverSearch({ page }).page).toBe(1);
    }
  });

  it('keeps a year inside the provider range and drops the rest', () => {
    expect(validateDiscoverSearch({ year: '1999' }).year).toBe(1999);
    for (const year of [1200, 2200, 'x']) {
      expect(validateDiscoverSearch({ year }).year).toBeUndefined();
    }
  });

  it('keeps only whole decades', () => {
    expect(validateDiscoverSearch({ decade: '1990' }).decade).toBe(1990);
    expect(validateDiscoverSearch({ decade: 1995 }).decade).toBeUndefined();
  });

  it('keeps a TMDB genre id', () => {
    expect(validateDiscoverSearch({ genre: '16' }).genre).toBe(16);
    expect(validateDiscoverSearch({ genre: 'sixteen' }).genre).toBeUndefined();
  });

  it('keeps a well-formed original-language code', () => {
    expect(validateDiscoverSearch({ lang: 'JA' }).lang).toBe('ja');
    expect(validateDiscoverSearch({ lang: 'japanese' }).lang).toBeUndefined();
  });

  it('keeps the TMDB ordering, defaulting to popularity', () => {
    expect(validateDiscoverSearch({ sort: 'rating' }).sort).toBe('rating');
    expect(validateDiscoverSearch({ sort: 'sideways' }).sort).toBe('popularity');
  });

  it('keeps the availability and watched filters, defaulting to any', () => {
    expect(validateDiscoverSearch({ state: 'out' }).state).toBe('out');
    expect(validateDiscoverSearch({ state: 'elsewhere' }).state).toBe('any');
    expect(validateDiscoverSearch({ watched: 'unwatched' }).watched).toBe('unwatched');
    expect(validateDiscoverSearch({ watched: 'sometimes' }).watched).toBe('any');
  });

  it('keeps a library id and a minimum rating', () => {
    expect(validateDiscoverSearch({ library: 'lib-anime' }).library).toBe('lib-anime');
    expect(validateDiscoverSearch({ rating: '7.5' }).rating).toBe(7.5);
    expect(validateDiscoverSearch({ rating: '11' }).rating).toBeUndefined();
  });

  it('OMITS a dropped key rather than setting it undefined', () => {
    const out = validateDiscoverSearch({ year: 'nope', library: '' });
    expect(Object.hasOwn(out, 'year')).toBe(false);
    expect(Object.hasOwn(out, 'library')).toBe(false);
  });

  it('keeps every valid filter together', () => {
    expect(
      validateDiscoverSearch({
        q: 'dune',
        type: 'movie',
        source: 'library',
        page: '2',
        year: 2024,
        genre: 878,
        library: 'lib-films',
        rating: 8,
        watched: 'unwatched',
        sort: 'rating',
        state: 'in',
        lang: 'en',
      }),
    ).toEqual(
      defaults({
        q: 'dune',
        type: 'movie',
        source: 'library',
        page: 2,
        year: 2024,
        genre: 878,
        library: 'lib-films',
        rating: 8,
        watched: 'unwatched',
        sort: 'rating',
        state: 'in',
        lang: 'en',
      }),
    );
  });

  it('ignores parameters it was never asked about', () => {
    expect(validateDiscoverSearch({ utm_source: 'x', q: 'dune' })).toEqual(defaults({ q: 'dune' }));
  });
});

describe('clearing the filters', () => {
  const filtered = validateDiscoverSearch({
    q: 'dune',
    source: 'library',
    year: 2024,
    genre: 878,
    library: 'lib-films',
    rating: 8,
    watched: 'unwatched',
    sort: 'rating',
  });

  it('counts every filter that is not at its default', () => {
    expect(activeFilterCount(filtered)).toBe(6);
    expect(activeFilterCount(DEFAULT_DISCOVER_SEARCH)).toBe(0);
  });

  it('resets the filters but keeps the query and tab', () => {
    expect(clearedDiscoverSearch(filtered)).toEqual(
      defaults({ q: 'dune', source: 'library' }),
    );
  });
});
