// @vitest-environment jsdom

import { MediaCard, MediaItem, Show } from '@kromatv/client/media';
import { I18nProvider } from '@kromatv/ui';
import { KitProvider } from '@kromatv/ui/kit';
import { declared } from '@kromatv/ui/testing';
import { createMemoryHistory, createRootRoute, createRouter, RouterContextProvider } from '@tanstack/react-router';
import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { type ReactNode, useState } from 'react';
import { afterEach, beforeAll, expect, it, vi } from 'vitest';
import { DetailHero, type DetailHeroProps } from './detail.component';
import { Hero } from './featured-hero.component';
import type { HomeEntry, HomeSection } from './home/entries';
import { HeroCarousel } from './home/hero-carousel.component';
import { Spotlight } from './home/spotlight.component';
import { HOLD_MS } from './home/use-carousel.hook';
import { RouteLink } from '#web/shared/ui/route-link.component';

const actions = vi.hoisted(() => ({ save: vi.fn(), theme: vi.fn() }));
vi.mock('#web/shared/lib/auth', () => ({
  useAuth: () => ({ client: { media: { artwork: {
    backdropFor: ({ id }: { id: string }, w?: number) => `/api/images/${id}.webp${w ? `?w=${w}` : ''}`,
  } } } }),
}));
vi.mock('#web/shared/lib/mylist', () => ({
  useMyList: () => ({ inList: () => false, toggle: actions.save }),
}));
vi.mock('@kromatv/ui', async (original) => ({
  ...await original<typeof import('@kromatv/ui')>(),
  useThemeAudio: (url: string | null) => ({ active: !!url, muted: false, toggle: actions.theme }),
  useCast: () => ({ available: true }),
}));

const intro = '猫妖小黑盗取天明珠后受伤逃走，被罗小白带回了家。直到有一天，小黑变成了人形，告诉小白自己要暂时离开。';
const common = { year: 2020, library: 'library', video: null, addedAt: '2020-01-01',
  metadata: { provider: 'tmdb', tmdbId: 1, title: null, tmdbUrl: '', overview: intro, genres: [], rating: 9.2 },
};
const show = { type: 'show', show: Show.parse({ ...common, id: 's1', title: '罗小黑战记', seasonCount: 1, episodeCount: 40 }) } as const;
const movie = { type: 'movie', item: MediaCard.parse({ ...common, kind: 'movie', id: 'm2', title: '雾山五行', durationMs: 5400000 }) } as const;
const section = (items: HomeEntry[] = [show, movie]): HomeSection => ({ id: 'hero', title: 'Home picks', layout: 'hero', notes: {}, items });
const router = createRouter({ routeTree: createRootRoute(), history: createMemoryHistory() });
beforeAll(() => router.load());
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  vi.clearAllMocks();
});

function mount(ui: ReactNode, width = 390) {
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue(new DOMRect(0, 0, width, 420));
  return render(ui, { wrapper: ({ children }) => (
    <RouterContextProvider router={router}>
      <KitProvider config={{ formFactor: 'browser', designWidth: width, ground: 'light' }}>
        <I18nProvider locale="en">{children}</I18nProvider>
      </KitProvider>
    </RouterContextProvider>
  ) });
}

const button = (name: string) => screen.getByRole('button', { name });
const shuffle = 'New picks';

it('only offers a batch action when supplied; a lone title has no arrows, timer stops or pause', () => {
  const change = vi.fn();
  const one = section([show]);
  const { rerender } = mount(<HeroCarousel section={one} />);
  expect(screen.queryByRole('button', { name: shuffle })).toBeNull();
  rerender(<HeroCarousel section={one} onShuffle={change} />);
  expect(screen.getAllByRole('button').map((b) => b.getAttribute('aria-label'))).toEqual(['Add to my list', shuffle]);
  expect(within(button(shuffle)).getByText(shuffle)).toBeTruthy();
  fireEvent.click(button(shuffle));
  expect(change).toHaveBeenCalledTimes(1);
  rerender(<HeroCarousel section={section([])} onShuffle={change} />);
  expect(screen.queryByRole('region')).toBeNull();
  expect(screen.queryByText(shuffle)).toBeNull();
});

it('pages movie and show actions, clamps the lead, and resets to the first stop on the caller’s new key', () => {
  const change = vi.fn();
  function Picks() {
    const [batch, setBatch] = useState(0);
    return <HeroCarousel key={batch} section={section()} onShuffle={() => { change(); setBatch(batch + 1); }} />;
  }
  mount(<Picks />, 1280);
  expect(declared(screen.getByText(intro), '-webkit-line-clamp')).toBe('2');
  expect(screen.getByRole('heading').closest('[data-theme="dark"]')).toBeNull();
  expect(screen.getByRole('link', { name: 'More info' }).getAttribute('href')).toBe('/shows/s1');
  const stop = button(`Show ${movie.item.title}`);
  expect(stop.querySelector('img')?.getAttribute('src')).toMatch(/m2\.webp\?w=240$/);
  expect(declared(stop, 'opacity')).not.toBe('0.55');
  fireEvent.click(stop);
  expect(stop.getAttribute('aria-pressed')).toBe('true');
  expect(screen.getByRole('link', { name: 'More info' }).getAttribute('href')).toBe('/movies/m2');
  expect(screen.getByRole('link', { name: 'Play' }).getAttribute('href')).toBe('/watch/m2');
  fireEvent.click(button('Add to my list'));
  expect(actions.save).toHaveBeenCalledWith('m2');
  fireEvent.click(button(shuffle));
  expect(change).toHaveBeenCalledTimes(1);
  expect(screen.getByRole('group', { name: '1 of 2' })).toBeTruthy();
  expect(screen.getByRole('heading').textContent).toBe(show.show.title);
  expect(screen.queryByRole('button', { name: /pause|resume/i })).toBeNull();
});

it('keeps narrow stops actionable, wraps arrows in both directions, and rests on hover and focus', async () => {
  vi.useFakeTimers();
  mount(<HeroCarousel section={section()} />, 320);
  const hero = screen.getByRole('region', { name: 'Home picks' });
  const heading = () => screen.getByRole('heading').textContent;
  expect(button(`Show ${movie.item.title}`).querySelector('img')).toBeNull();
  fireEvent.click(button('Previous title'));
  expect(heading()).toBe(movie.item.title);
  fireEvent.click(button('Next title'));
  expect(heading()).toBe(show.show.title);
  fireEvent.keyDown(hero, { key: 'ArrowLeft' });
  expect(heading()).toBe(movie.item.title);
  fireEvent.keyDown(hero, { key: 'ArrowRight' });
  fireEvent.pointerEnter(hero);
  await act(() => vi.advanceTimersByTimeAsync(HOLD_MS));
  expect(heading()).toBe(show.show.title);
  fireEvent.focusIn(button('Next title'));
  fireEvent.pointerLeave(hero);
  await act(() => vi.advanceTimersByTimeAsync(HOLD_MS));
  expect(heading()).toBe(show.show.title);
  fireEvent.focusOut(button('Next title'));
  await act(() => vi.advanceTimersByTimeAsync(HOLD_MS));
  expect(heading()).toBe(movie.item.title);
});

it('does not auto-advance under reduced motion; manual paging still works', async () => {
  vi.useFakeTimers();
  vi.stubGlobal('matchMedia', (query: string) => Object.assign(new EventTarget(), { matches: query.includes('reduced-motion') }));
  mount(<HeroCarousel section={section()} />);
  await act(() => vi.advanceTimersByTimeAsync(HOLD_MS));
  expect(screen.getByRole('heading').textContent).toBe(show.show.title);
  fireEvent.click(button('Next title'));
  expect(screen.getByRole('heading').textContent).toBe(movie.item.title);
});

it('keeps the fallback and spotlight on the page ground, with a two-line lead and no batch actions', () => {
  const { rerender } = mount(<Hero entry={{ type: 'movie', movie: { ...movie.item, poster: 'cover', backdrop: null } }} />);
  expect(screen.getByRole('link', { name: 'Play' }).getAttribute('href')).toBe('/watch/m2');
  expect(declared(screen.getByText(intro), '-webkit-line-clamp')).toBe('2');
  rerender(<Spotlight section={{ ...section([show]), reason: 'Because you watched', layout: 'spotlight' }} />);
  expect(screen.getByText('Because you watched')).toBeTruthy();
  expect(screen.getByRole('heading').closest('[data-theme="dark"]')).toBeNull();
  expect(screen.getByRole('link', { name: 'More info' }).getAttribute('href')).toBe('/shows/s1');
  expect(declared(screen.getByText(intro), '-webkit-line-clamp')).toBe('2');
  expect(screen.queryByRole('button')).toBeNull();
});

const detail: DetailHeroProps = {
  art: { id: 's1', backdrop: '/api/images/detail.webp', poster: '/api/images/cover.webp' },
  title: '一念永恒', overline: '动画 · 科幻与奇幻', rating: 8.9, meta: '2020 · 1 season · 184 episodes',
  badges: ['HDR'], back: <RouteLink to="/" />, tagline: '长生之愿一念永恒！', overview: intro.repeat(8),
};

it('keeps only the scenic title and corner actions on the art; the poster and complete fiche follow even on a phone', () => {
  const play = vi.fn(), watched = vi.fn(), list = vi.fn(), report = vi.fn();
  const { container } = mount(<DetailHero {...detail}
    playable={MediaItem.parse({ ...movie.item, showId: null, showTitle: null, season: null, episode: null, episodeEnd: null, episodeTitle: null, container: 'mkv', relPath: null, audio: null, audioTracks: [], subtitles: [], files: [], video: { codec: 'unknown', width: null, height: null, hdr: false, bitDepth: null } })}
    castItemId={movie.item.id} themeUrl="theme.mp3" audio="Mandarin" subtitles="None"
    directors={[{ name: '耳根', job: 'Director', tmdbId: 12 }]}
    playLabel="Play S1E1" onPlay={play} watched onToggleWatched={watched} inList onToggleList={list} onReport={report}
    trailerAction={<button type="button">Trailer</button>} adminAction={<button type="button">Change artwork</button>}
  />, 320);
  const mast = container.querySelector('header')!;
  const body = mast.nextElementSibling as HTMLElement;
  expect(within(mast).getByRole('heading', { level: 1 }).textContent).toBe(detail.title);
  expect(within(mast).getByRole('link', { name: 'Back' }).getAttribute('href')).toBe('/');
  fireEvent.click(within(mast).getByRole('button', { name: 'Mute theme music' }));
  expect(actions.theme).toHaveBeenCalledOnce();
  expect(mast.querySelectorAll('img')).toHaveLength(1);
  expect(body.querySelector('img')?.src).toMatch(/cover\.webp$/);
  const fiche = within(body);
  const primary = fiche.getByRole('button', { name: 'Play S1E1' });
  for (const name of ['Watched', 'Remove from my list', 'Report a problem', 'Play S1E1']) fireEvent.click(fiche.getByRole('button', { name }));
  for (const action of [play, watched, list, report]) expect(action).toHaveBeenCalledOnce();
  for (const name of ['Play on TV', 'Change artwork', 'Trailer']) expect(fiche.getByRole('button', { name })).toBeTruthy();
  const director = fiche.getByRole('link', { name: 'See titles with 耳根' });
  const synopsis = fiche.getAllByText(detail.overview!)[0]!;
  expect(primary.compareDocumentPosition(synopsis) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(synopsis.compareDocumentPosition(director) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  for (const value of ['HDR', 'Mandarin', 'None', detail.tagline!]) expect(fiche.getByText(value)).toBeTruthy();
  expect(body.querySelector('[data-theme="dark"]')).toBeNull();
});

it('does not invent gated actions and respects a supplied primary action', () => {
  const { rerender } = mount(<DetailHero {...detail} />);
  expect(screen.queryAllByRole('button')).toHaveLength(0);
  rerender(<DetailHero {...detail} onPlay={vi.fn()} primaryAction={<button type="button">Request</button>} />);
  expect(screen.getAllByRole('button')).toEqual([button('Request')]);
});
