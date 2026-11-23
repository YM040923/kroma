// @vitest-environment jsdom

import type { ItemId, SubjectId } from '@kromatv/client/media';
import { I18nProvider } from '@kromatv/ui';
import { QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterAll, afterEach, describe, expect, it, vi } from 'vitest';
import {
  defaultSeasonNumber,
  missingEpisodeNumbers,
  SeasonSection,
} from '#web/features/catalog/episode-list.component';
import { WebRuntimeProvider } from '#web/shared/lib/web-runtime';
import { createWebTestRuntime } from '#web/shared/lib/web-runtime.fixture';
import type { TitleSeason } from '#web/shared/lib/title-view';

vi.hoisted(() => {
  vi.stubEnv('NODE_ENV', 'development');
});

afterAll(() => {
  vi.unstubAllEnvs();
});

afterEach(cleanup);

const ep = (season: number, episode: number, title = `Ep ${episode}`) =>
  ({
    id: `e${season}-${episode}`,
    title,
    season,
    episode,
    video: {},
    metadata: null,
  }) as never;

function season(number: number, over: Partial<TitleSeason> = {}): TitleSeason {
  const episodes = over.episodes ?? [];
  return {
    number,
    name: null,
    episodeCount: episodes.length,
    episodesAvailable: episodes.length,
    available: true,
    requested: false,
    airDate: null,
    episodes,
    missing: [],
    cast: [],
    ...over,
  };
}

const s1 = season(1, { episodes: [ep(1, 1), ep(1, 2)] });
const s2 = season(2, {
  name: 'Season Two',
  episodeCount: 3,
  episodesAvailable: 0,
  available: false,
  missing: [
    {
      episode: 1,
      title: 'Pilot',
      overview: 'A beginning.',
      airDate: '2030-01-01',
    },
  ],
});
const s23 = season(23, { episodes: [ep(23, 1)] });
const partial = season(5, {
  episodeCount: 5,
  episodesAvailable: 1,
  available: false,
  episodes: [ep(5, 1)],
  missing: [{ episode: 2, title: 'Gap', airDate: '2031-01-01' }],
});

function mount(over: Partial<Parameters<typeof SeasonSection>[0]> = {}) {
  const runtime = createWebTestRuntime({});
  const props: Parameters<typeof SeasonSection>[0] = {
    seasons: [s1, s2, s23],
    poster: null,
    overview: 'A crew hunts a treasure.',
    year: 1999,
    discoverTmdbId: null,
    fallbackCast: [],
    isWatched: () => false,
    toggleWatched: () => {},
    progressOf: () => null,
    onPlay: () => {},
    ...over,
  };
  return render(
    <QueryClientProvider client={runtime.queryClient}>
      <WebRuntimeProvider value={runtime}>
        <I18nProvider locale="en">
          <SeasonSection {...props} />
        </I18nProvider>
      </WebRuntimeProvider>
    </QueryClientProvider>,
  );
}

describe('default season', () => {
  it('opens on a season the library holds, never on TMDB-only season 1', () => {
    expect(defaultSeasonNumber([s1, s2, s23], undefined, () => false, () => null)).toBe(1);
  });

  it('prefers the most recently watched local season', () => {
    expect(
      defaultSeasonNumber([s1, s2, s23], undefined, () => false, (id) =>
        id === ('e23-1' as ItemId) ? 12 : null,
      ),
    ).toBe(23);
    expect(
      defaultSeasonNumber([s1, s2, s23], undefined, (id) => id === ('e1-2' as SubjectId), () => null),
    ).toBe(1);
  });

  it('honours an explicit local season, but not an explicit missing one', () => {
    expect(defaultSeasonNumber([s1, s2, s23], 23, () => false, () => null)).toBe(23);
    expect(defaultSeasonNumber([s1, s2, s23], 2, () => false, () => null)).toBe(1);
  });

  it('enters the missing view only when nothing is local', () => {
    expect(defaultSeasonNumber([s2], undefined, () => false, () => null)).toBe(2);
  });
});

describe('missing episodes of a season', () => {
  it('enumerates an unowned season up to its TMDB count', () => {
    expect(missingEpisodeNumbers(s2, true)).toEqual([1, 2, 3]);
  });

  it('subtracts the owned episodes of a partially held season', () => {
    expect(missingEpisodeNumbers(partial, true)).toEqual([2, 3, 4, 5]);
  });

  it('never lists gaps for a fully available season', () => {
    expect(missingEpisodeNumbers(s1, true)).toEqual([]);
  });

  it('falls back to the stored guide without request permission', () => {
    expect(missingEpisodeNumbers(partial, false)).toEqual([2]);
    expect(missingEpisodeNumbers(s2, false)).toEqual([1]);
  });
});

describe('the season section', () => {
  it('draws only owned seasons until the viewer asks for the rest', () => {
    mount();

    expect(screen.getByText('Season 1')).toBeTruthy();
    expect(screen.getByText('Season 23')).toBeTruthy();
    expect(screen.queryByText('Season 2 · Not in library')).toBeNull();
    expect(screen.getByText('All seasons (incl. missing)')).toBeTruthy();
    expect(screen.getByText('1. Ep 1')).toBeTruthy();
    expect(screen.queryByText('Season Two')).toBeNull();
  });

  it('shows missing seasons as unstored and does not select one on its own', () => {
    mount();

    fireEvent.click(screen.getByText('All seasons (incl. missing)'));
    expect(screen.getByText('Season 2 · Not in library')).toBeTruthy();
    expect(screen.queryByText('Season Two')).toBeNull();
    expect(screen.getByText('1. Ep 1')).toBeTruthy();

    fireEvent.click(screen.getByText('Season 2 · Not in library'));
    expect(screen.getByText('Season Two')).toBeTruthy();
    expect(screen.getByText(/1999 · 3 episodes/)).toBeTruthy();
    expect(screen.getByText('A crew hunts a treasure.')).toBeTruthy();
    expect(screen.getByText('1. Pilot')).toBeTruthy();
    expect(screen.getByText('A beginning.')).toBeTruthy();
    expect(screen.getByText('Episode 2')).toBeTruthy();
    expect(screen.getAllByText('Not in library').length).toBeGreaterThanOrEqual(2);
  });

  it('folds the gaps of a partially owned season behind one entry', () => {
    mount({ seasons: [partial] });

    expect(screen.getByText('1. Ep 1')).toBeTruthy();
    expect(screen.queryByText('2. Gap')).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: 'Show the 4 missing episodes' }));
    expect(screen.getByText('2. Gap')).toBeTruthy();
    expect(screen.getByText('Episode 3')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Hide the 4 missing episodes' })).toBeTruthy();
  });

  it('offers no whole-season request for a season already available', () => {
    mount();

    expect(screen.queryByText(/Request season 1/)).toBeNull();
    expect(screen.getByText('Request all missing seasons · 1 season')).toBeTruthy();
  });

  it('offers a scoped whole-season request while a missing season is open', () => {
    mount();

    fireEvent.click(screen.getByText('All seasons (incl. missing)'));
    fireEvent.click(screen.getByText('Season 2 · Not in library'));

    expect(screen.getByText('Request season 2 · 3 episodes')).toBeTruthy();
    expect(screen.getByText('Episodes already owned or requested are skipped.')).toBeTruthy();
  });
});
