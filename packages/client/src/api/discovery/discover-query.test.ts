import { describe, expect, it } from 'vitest';
import { queryString } from '../../core/http';
import {
  discoverFilterQuery,
  discoverQuery,
  localDiscoverQuery,
} from './client';

describe('discoverQuery', () => {
  it('asks for nothing when nothing narrows the listing', () => {
    expect(queryString(discoverQuery())).toBe('');
    expect(queryString(discoverQuery({}))).toBe('');
  });

  it('reads "all" as the absence of a type filter', () => {
    expect(queryString(discoverQuery({ type: 'all' }))).toBe('');
    expect(queryString(discoverQuery({ type: 'tv' }))).toBe('?type=tv');
  });

  it('reads page one as the absence of a page', () => {
    expect(queryString(discoverQuery({ page: 1 }))).toBe('');
    expect(queryString(discoverQuery({ page: 3 }))).toBe('?page=3');
  });

  it('drops the any availability filter', () => {
    expect(queryString(discoverQuery({ state: 'any' }))).toBe('');
    expect(queryString(discoverQuery({ state: 'out' }))).toBe('?state=out');
  });
});

describe('discoverFilterQuery', () => {
  it('sends only the narrowers, dropping popularity/any/page one', () => {
    expect(
      queryString(
        discoverFilterQuery({
          type: 'movie',
          sort: 'popularity',
          state: 'any',
          page: 1,
        }),
      ),
    ).toBe('?type=movie');
  });

  it('carries sort, language, year, genre and availability', () => {
    expect(
      queryString(
        discoverFilterQuery({
          type: 'tv',
          sort: 'rating',
          language: 'ja',
          decade: 1990,
          genre: 16,
          page: 2,
          state: 'in',
        }),
      ),
    ).toBe('?type=tv&sort=rating&language=ja&decade=1990&genre=16&page=2&state=in');
  });
});

describe('localDiscoverQuery', () => {
  it('asks for everything when nothing narrows the local grid', () => {
    expect(queryString(localDiscoverQuery({ type: 'all', watched: 'any' }))).toBe('');
  });

  it('carries the library, year, genre, rating and watched filters', () => {
    expect(
      queryString(
        localDiscoverQuery({
          q: 'dune',
          type: 'movie',
          library: 'lib1',
          year: 2024,
          genre: 878,
          rating: 7.5,
          watched: 'unwatched',
          page: 2,
          perPage: 24,
        }),
      ),
    ).toBe(
      '?q=dune&type=movie&library=lib1&year=2024&genre=878&rating=7.5&watched=unwatched&page=2&perPage=24',
    );
  });
});
