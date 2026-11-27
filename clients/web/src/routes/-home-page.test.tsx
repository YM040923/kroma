// @vitest-environment jsdom

import type { CardSection } from '@kromatv/client/media';
import { I18nProvider } from '@kromatv/ui';
import { QueryClientProvider } from '@tanstack/react-query';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import type { ComponentType, ReactNode } from 'react';
import { afterEach, expect, it, vi } from 'vitest';
import type { HeroEntry } from '#web/shared/lib/queries';
import { WebRuntimeProvider } from '#web/shared/lib/web-runtime';
import { createWebTestRuntime } from '#web/shared/lib/web-runtime.fixture';

const session = vi.hoisted(() => ({ signedIn: true }));

vi.mock('#web/shared/lib/auth', () => ({
  useAuth: () => ({ ready: true, user: session.signedIn ? { id: 'u1' } : null }),
}));
vi.mock('#web/features/catalog/featured-hero.component', () => ({
  Hero: ({ entry }: { entry: HeroEntry }) => (
    <p>{`hero ${entry.type === 'movie' ? entry.movie.id : entry.show.id}`}</p>
  ),
  HeroSkeleton: () => <p>hero pending</p>,
}));
vi.mock('#web/features/catalog/home/hero-carousel.component', () => ({
  HeroCarousel: ({ section, onShuffle }: { section: CardSection; onShuffle?: () => void }) => (
    <div><p>{`carousel of ${section.items.length}`}</p><output data-testid="hero-ids">{section.items.map(e => e.type === 'show' ? e.show.id : e.item.id).join(',')}</output>{onShuffle && <button onClick={onShuffle}>shuffle</button>}</div>
  ),
}));
vi.mock('#web/features/catalog/continue-row.component', () => ({
  ContinueRow: () => <p>continue</p>,
}));
vi.mock('#web/features/catalog/home/home-sections.component', () => ({
  HomeSectionView: ({ section }: { section: CardSection }) => <p>{`section ${section.id}`}</p>,
  SectionsSkeleton: () => <p>sections pending</p>,
}));

vi.mock('@tanstack/react-router', async (load) => ({
  ...await load<typeof import('@tanstack/react-router')>(),
  Link: ({to, params, children}: {to: string; params: {id: string}; children: ReactNode}) => <a href={to.replace('$id', params.id)}>{children}</a>,
}));
vi.mock('#web/shared/ui', async (load) => ({
  ...await load<typeof import('#web/shared/ui')>(),
  Poster: ({title, children}: {title: string; children: ReactNode}) => <div>{title}{children}</div>,
  PosterRail: <T,>({data, renderItem}: {data: T[]; renderItem: (item: T) => ReactNode}) => <div>{data.map((item, i) => <span key={i}>{renderItem(item)}</span>)}</div>,
}));
vi.mock('#web/shared/ui/route-link.component', () => ({
  RouteLink: ({to, params, search}: {to: string; params?: {id: string}; search?: {photo?: string}}) =>
    <a href={to.replace('$id', params?.id ?? '') + (search?.photo ? `?photo=${encodeURIComponent(search.photo)}` : '')} />,
}));
vi.mock('#web/features/catalog/cards.component', () => ({
  MoviePoster: ({item}: {item: {id: string}}) => <b>{`film ${item.id}`}</b>,
  ShowPoster: ({show}: {show: {id: string}}) => <b>{`series ${show.id}`}</b>,
}));

import { Route } from './_app.index';

const art = { photoUrl: () => '/photo', posterFor: () => '/poster', showPosterFor: () => '/poster', backdropFor: () => null };

const show = (id: string) => ({
  type: 'show',
  show: {
    id,
    title: id,
    year: null,
    library: 'lib',
    video: null,
    addedAt: 't',
    seasonCount: 1,
    episodeCount: 1,
  },
});

const section = (id: string, layout: string, items: unknown[]) => ({
  id,
  title: id,
  layout,
  items,
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  session.signedIn = true;
});

interface EventSocket {
  onmessage: ((event: MessageEvent) => void) | null;
}

function openedSockets(): EventSocket[] {
  const sockets: EventSocket[] = [];
  class Socket implements EventSocket {
    onmessage: ((event: MessageEvent) => void) | null = null;
    close = vi.fn();
    constructor() {
      sockets.push(this);
    }
  }
  vi.stubGlobal('WebSocket', Socket);
  return sockets;
}

const announce = (socket: EventSocket | undefined, frame: object) =>
  socket?.onmessage?.(new MessageEvent('message', { data: JSON.stringify(frame) }));

async function mount(
  home: unknown[] | Promise<never>,
  media: { libraries?: () => Promise<unknown>; showCards?: (library?: string) => Promise<unknown>; featured?: () => Promise<unknown>; movieCards?: (library?: string) => Promise<unknown> } = {},
  credentials: { sessionToken?: string } = {},
  layout: { order: string[]; hidden: string[] } = { order: [], hidden: [] },
  discovery: ReturnType<typeof show>[] = [],
) {
  const fetched = vi.fn(async (url: RequestInfo | URL) => {
    const path = new URL(String(url)).pathname;
    if (path === '/api/auth/me/home-layout') return Response.json(layout);
    if (path === '/api/continue') return Response.json([]);
    if (path === '/api/photos' || path === '/api/other') {
      const library = new URL(String(url)).searchParams.get('library');
      if (library === 'restricted') return Response.json([]);
      return Response.json([{ id:'file', title:`file in ${library}`, library, kind:path.endsWith('photos') ? 'photo':'other',
        year:null, video:null, addedAt:'2026-01-01', audio:null, audioTracks:[], subtitles:[], container:'', durationMs:null,
        relPath:null, showId:null, showTitle:null, season:null, episode:null, episodeEnd:null, episodeTitle:null, files:[] }]);
    }
    return Response.json(await home);
  });
  vi.stubGlobal('fetch', fetched);
  const runtime = createWebTestRuntime({
    ...credentials,
    media: {
      libraries: async () => [],
      showCards: async () => [],
      movieCards: async () => [],
      featured: async () => null,
      homeDiscovery: async () => discovery,
      artwork: art,
      ...media,
    },
  });
  const Page = Route.options.component as ComponentType;
  await act(async () => {
    render(
      <WebRuntimeProvider value={runtime}>
        <QueryClientProvider client={runtime.queryClient}>
          <I18nProvider locale="en">
            <Page />
          </I18nProvider>
        </QueryClientProvider>
      </WebRuntimeProvider>,
    );
  });
  return fetched;
}

it('draws the continue row while the home is still on its way', async () => {
  await mount(new Promise<never>(() => undefined));

  expect(await screen.findByText('continue')).toBeTruthy();
  expect(screen.getByText('hero pending')).toBeTruthy();
});

it('opens one scoped mixed rail near the viewport, merges by date and skips hidden and empty libraries', async () => {
  const callbacks: (() => void)[] = [];
  vi.stubGlobal('IntersectionObserver', class {
    constructor(cb: (entries: {isIntersecting: boolean}[]) => void) { callbacks.push(() => cb([{isIntersecting:true}])); }
    observe() {} disconnect() {}
  });
  const lib = {id:'mixed', name:'Weekend', kind:'mixed', itemCount:3, path:''};
  const movieCards = vi.fn(async () => [
    {id:'old', addedAt:'2025-01-01', library:'mixed'}, {id:'foreign', addedAt:'2026-01-01', library:'another'},
  ]);
  const showCards = vi.fn(async () => [{id:'new', addedAt:'2026-01-01', library:'mixed'}]);
  await mount([], {movieCards, showCards, libraries: async () => [lib, {...lib,id:'empty',itemCount:0}, {...lib,id:'hidden'}]}, {}, {hidden:['hero','lib:hidden'],order:[]});
  await vi.waitFor(() => expect(callbacks).toHaveLength(1));
  expect(movieCards).not.toHaveBeenCalled();
  act(() => callbacks[0]!());
  const title = await screen.findByRole('heading', {name:'Weekend'});
  expect(title.querySelector('a')?.getAttribute('href')).toBe('/libraries/mixed');
  expect(screen.getAllByRole('link').map(a => a.getAttribute('href'))).toEqual(['/libraries/mixed','/libraries/mixed']);
  expect(movieCards.mock.calls).toEqual([['mixed']]);
  expect(showCards.mock.calls).toEqual([['mixed']]);
  expect(screen.getByText('series new').compareDocumentPosition(screen.getByText('film old')) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(screen.queryByText('film foreign')).toBeNull();
});

it('keeps photos and other files out of film details, and omits headings for filtered-out content', async () => {
  const libraries = ['photos','other','restricted'].map(id => ({id, name:`Shelf ${id}`, kind:id === 'other' ? 'other' : 'photos', path:'', itemCount:3}));
  const fetched = await mount([], { libraries: async () => libraries }, {}, {hidden:['hero'],order:[]});
  const photo = await screen.findByText('file in photos');
  const other = await screen.findByText('file in other');
  expect(photo.querySelector('a')?.getAttribute('href')).toBe('/libraries/photos?photo=file');
  expect(other.querySelector('a')?.getAttribute('href')).toBe('/watch/file');
  expect(fetched.mock.calls.some(([url]) => String(url).includes('library=restricted'))).toBe(true);
  expect(screen.queryByText('Shelf restricted')).toBeNull();
});

it('opens on the hero the server sent', async () => {
  await mount([section('hero', 'hero', [show('s1'), show('s2')]), section('recent', 'rail', [show('s3')])]);

  expect(await screen.findByText('carousel of 2')).toBeTruthy();
  expect(await screen.findByText('section recent')).toBeTruthy();
});

it('swaps real titles without turning the page arrows into a shuffle', async () => {
  const entries = Array.from({length:12}, (_,n) => show(`s${n}`));
  await mount([section('hero','hero',entries.slice(0,5)), section('spotlight','spotlight',[entries[5]])], {}, {}, {order:[],hidden:[]}, entries);
  const shuffle = await screen.findByText('shuffle');
  expect(screen.getByTestId('hero-ids').textContent).toBe('s0,s1,s2,s3,s4');
  fireEvent.click(shuffle);
  expect(screen.getByTestId('hero-ids').textContent).toBe('s6,s7,s8,s9,s10');
});

it('respects ordering across the old fixed slots and never restores a disabled hero via the featured fallback', async () => {
  const featured = vi.fn(async () => null);
  await mount([section('recent','rail',[show('s1')]), section('spotlight','spotlight',[show('s2')])], {featured}, {}, {order:['recent','spotlight','continue'],hidden:['hero']});
  const recent = await screen.findByText('section recent');
  const spot = await screen.findByText('section spotlight');
  expect(recent.compareDocumentPosition(spot) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(spot.compareDocumentPosition(screen.getByText('continue')) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(featured).not.toHaveBeenCalled();
});

it('falls back to the pick of the day for a server that sends no hero', async () => {
  await mount([section('recent', 'rail', [show('s1')])], {
    featured: async () => ({ type: 'show', show: { id: 's9' } }),
  });

  expect(await screen.findByText('hero s9')).toBeTruthy();
});

it('falls back to the first movie when the server has no pick either', async () => {
  await mount([], { movieCards: async () => [{ id: 'm1' }, { id: 'm2' }] });

  expect(await screen.findByText('hero m1')).toBeTruthy();
});

it('says the library is empty when there is nothing at all to show', async () => {
  await mount([]);

  expect(await screen.findByText('Your library is empty.')).toBeTruthy();
});

it('asks for nothing of the home before somebody is signed in', async () => {
  session.signedIn = false;

  const fetched = await mount([]);

  expect(screen.getByText('hero pending')).toBeTruthy();
  expect(screen.getByText('sections pending')).toBeTruthy();
  expect(fetched).not.toHaveBeenCalled();
});

it('replaces the empty library with the first title a scan found, without a reload', async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  const sockets = openedSockets();
  const movies: unknown[] = [];
  await mount([], { movieCards: async () => movies }, { sessionToken: 'bearer' });
  expect(await screen.findByText('Your library is empty.')).toBeTruthy();

  movies.push({ id: 'm1' });
  announce(sockets[0], { type: 'scan.completed', items: 1, shows: 0, libraries: 1 });
  await act(() => vi.advanceTimersByTimeAsync(2500));

  expect(await screen.findByText('hero m1')).toBeTruthy();
  expect(screen.queryByText('Your library is empty.')).toBeNull();
});
