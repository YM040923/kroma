import { ItemId, ShowId } from '@kromatv/client/media';
import { describe, expect, it, vi } from 'vitest';
import { c, installHarness, run, show } from './queries.fixture';

vi.mock('#web/shared/lib/api', () => ({
  kromaClient: () => c,
  toMovieView: (_c: unknown, m: { id: string }) => ({ ...m, mapped: 'movie' }),
  toShowView: (_c: unknown, s: { id: string }) => ({ ...s, mapped: 'show' }),
}));

const { createQueries } = await import('#web/shared/lib/queries');
const { catalogQueries } = createQueries(c);

installHarness();

const detail = (id: string, genres: string[], tmdbId: number | null = null) => ({
  show: show(id, genres, tmdbId),
});

describe('the show bundle', () => {
  it('prefers shows that share a genre', async () => {
    c.media.show.mockResolvedValue(detail('s1', ['Drama']));
    c.media.showCards.mockResolvedValue([
      show('s1', ['Drama']),
      show('a', ['Drama']),
      show('b', ['Drama']),
      show('c', ['Drama']),
      show('z', ['Comedy']),
    ]);

    const { similarShows } = await run(catalogQueries.showBundle(ShowId.parse('s1')));
    expect(similarShows.map((s: { id: string }) => s.id)).toEqual(['a', 'b', 'c']);
  });

  it('tolerates a show, and a catalogue, with no metadata at all', async () => {
    c.media.show.mockResolvedValue({ show: { id: 's1' } });
    c.media.showCards.mockResolvedValue([{ id: 's1' }, { id: 'a' }, { id: 'b' }]);

    const { similarShows } = await run(catalogQueries.showBundle(ShowId.parse('s1')));
    expect(similarShows.map((s: { id: string }) => s.id)).toEqual(['a', 'b']);
  });

  it('never suggests the show you are already looking at', async () => {
    c.media.show.mockResolvedValue(detail('s1', ['Drama']));
    c.media.showCards.mockResolvedValue([show('s1', ['Drama']), show('a', ['Drama'])]);

    const { similarShows } = await run(catalogQueries.showBundle(ShowId.parse('s1')));
    expect(similarShows.map((s: { id: string }) => s.id)).not.toContain('s1');
  });

  it('falls back to the rest of the catalogue rather than showing one lonely card', async () => {
    c.media.show.mockResolvedValue(detail('s1', ['Drama']));
    c.media.showCards.mockResolvedValue([
      show('s1', ['Drama']),
      show('a', ['Drama']),
      show('x', ['Comedy']),
      show('y', ['Horror']),
    ]);

    const { similarShows } = await run(catalogQueries.showBundle(ShowId.parse('s1')));
    expect(similarShows.map((s: { id: string }) => s.id)).toEqual(['a', 'x', 'y']);
  });

  it('caps the rail so a big library does not render hundreds of cards', async () => {
    c.media.show.mockResolvedValue(detail('s1', ['Drama']));
    c.media.showCards.mockResolvedValue([
      show('s1', ['Drama']),
      ...Array.from({ length: 30 }, (_, i) => show(`o${i}`, ['Drama'])),
    ]);

    const { similarShows } = await run(catalogQueries.showBundle(ShowId.parse('s1')));
    expect(similarShows).toHaveLength(12);
  });

  it('skips the discover overlay entirely for a show TMDB never matched', async () => {
    c.media.show.mockResolvedValue(detail('s1', [], null));

    const { discover } = await run(catalogQueries.showBundle(ShowId.parse('s1')));
    expect(discover).toBeNull();
    expect(c.discovery.detail).not.toHaveBeenCalled();
  });

  it('asks for the overlay by the show tmdb id when there is one', async () => {
    c.media.show.mockResolvedValue(detail('s1', [], 1396));
    c.discovery.detail.mockResolvedValue({ seasons: [] });

    const { discover } = await run(catalogQueries.showBundle(ShowId.parse('s1')));
    expect(c.discovery.detail).toHaveBeenCalledWith('tv', 1396);
    expect(discover).toEqual({ seasons: [] });
  });

  it('still renders the page for a viewer who may not request', async () => {
    c.media.show.mockResolvedValue(detail('s1', [], 1396));
    c.discovery.detail.mockRejectedValue(new Error('403'));
    c.playback.upNext.mockRejectedValue(new Error('403'));

    const bundle = await run(catalogQueries.showBundle(ShowId.parse('s1')));
    expect(bundle.discover).toBeNull();
    expect(bundle.upNext).toBeNull();
    expect(bundle.detail).toBeTruthy();
  });
});

describe('the optional lookups', () => {
  it('treat a missing similar list as no suggestions', async () => {
    c.media.similarCards.mockRejectedValue(new Error('boom'));
    expect(await run(catalogQueries.similar(ItemId.parse('m1')))).toEqual([]);
  });

  it('keep the credit name when the person profile cannot be fetched', async () => {
    c.media.person.mockRejectedValue(new Error('boom'));
    expect(await run(catalogQueries.personDetails('Greta Gerwig'))).toEqual({
      name: 'Greta Gerwig',
      person: null,
      credits: [],
    });
  });
});

describe('the watch queries', () => {
  it('reads the raw detail without waiting for upcoming episodes', async () => {
    c.media.item.mockResolvedValue({ id: 'e1' });
    c.playback.following.mockResolvedValue([{ id: 'e2' }, { id: 'e3' }]);

    const out = await run(catalogQueries.item(ItemId.parse('e1')));
    expect(out).toEqual({ id: 'e1' });
    expect(c.playback.following).not.toHaveBeenCalled();
    expect(await run(catalogQueries.following(ItemId.parse('e1')))).toHaveLength(2);
  });

  it('keys resume positions by viewer as well as item', () => {
    const first = catalogQueries.resumeProgress(ItemId.parse('e1'), 'u1');
    const second = catalogQueries.resumeProgress(ItemId.parse('e1'), 'u2');
    expect(first.queryKey).not.toEqual(second.queryKey);
  });
});
