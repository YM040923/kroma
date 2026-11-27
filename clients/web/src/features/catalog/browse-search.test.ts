import { expect, it } from 'vitest';
import { browseDestination } from './browse-search';

it('preserves the raw library ID and prioritizes its scope over genre', () => {
  expect(browseDestination({ library: '电影 %201', genre: 'drama', sort: 'rating' }))
    .toEqual({ to: '/libraries/$id', params: { id: '电影 %201' } });
});

it('opens the genre route for an old genre-only link', () => {
  expect(browseDestination({ genre: 'science-fiction' }))
    .toEqual({ to: '/genres/$id', params: { id: 'science-fiction' } });
});

it('does not turn a sort or unrelated parameter into navigation', () => {
  expect(browseDestination({ sort: 'rating', library: '', page: 2 })).toBeUndefined();
});
