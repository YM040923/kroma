// @vitest-environment jsdom
import { Library } from '@kromatv/client/media';
import { I18nProvider } from '@kromatv/ui';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRouter, RouterContextProvider } from '@tanstack/react-router';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import type { ComponentType } from 'react';
import { afterEach, beforeAll, expect, it, vi } from 'vitest';
import { createWebTestRuntime } from '#web/shared/lib/web-runtime.fixture';
import { WebRuntimeProvider } from '#web/shared/lib/web-runtime';
import { Route } from './_app.libraries.$id';

vi.mock('#web/shared/lib/auth', () => ({ useAuth: () => ({ user: { id: 'viewer' }, client: { media: { artwork: { photoUrl: (id: string) => `/api/items/${id}/photo` } } } }) }));
vi.mock('#web/shared/lib/watched', () => ({ useWatched: () => ({ isWatched: () => false }) }));
vi.mock('#web/shared/lib/mylist', () => ({ useMyList: () => ({ inList: () => false }) }));
vi.mock('@kromatv/ui', async (load) => ({ ...await load<typeof import('@kromatv/ui')>(), useT: () => (key: string) => key }));

const router = createRouter({ routeTree: createRootRoute(), history: createMemoryHistory() });
beforeAll(() => router.load());
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

const id = 'lib % 剧';
const grant = Library.parse({ id, name: '家庭影院', kind: 'mixed', path: '/', itemCount: 2 });
function mount(libraries: () => Promise<unknown[]>, movieCards = vi.fn(async (_id: string) => [] as unknown[]), warm = false, photo?: string) {
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} });
  vi.spyOn(Element.prototype, 'clientWidth', 'get').mockReturnValue(1024);
  vi.spyOn(Route, 'useParams').mockReturnValue({ id });
  vi.spyOn(Route, 'useSearch').mockReturnValue(photo ? { photo } : {});
  const showCards = vi.fn(async (_id: string) => [{ id: 'series', title: 'Series', addedAt: '2026-01-01', seasonCount: 2 }]);
  const cache = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const runtime = createWebTestRuntime({ media: { libraries, movieCards, showCards, artwork: {
    posterFor: () => null, showPosterFor: () => null, backdropFor: () => null,
  } } }, () => cache);
  if (warm) cache.setQueryData(runtime.queries.catalogQueries.libraries('viewer').queryKey, [grant]);
  const Page = Route.options.component as ComponentType;
  render(<RouterContextProvider router={router}><WebRuntimeProvider value={runtime}>
    <QueryClientProvider client={cache}><I18nProvider locale="en"><Page /></I18nProvider></QueryClientProvider>
  </WebRuntimeProvider></RouterContextProvider>);
  return { movieCards, showCards };
}

it.each(['movies', 'shows'] as const)('renders the %s overview newest first with no library, genre or sort controls', async (kind) => {
  const overview = kind === 'movies' ? await import('./_app.movies.index') : await import('./_app.shows.index');
  const rows = ['older','newer'].map((name, i) => ({ id: name, title: name, library: 'one', year: 2023,
    addedAt: `${2020+i}-01-01`, seasonCount: 1, episodeCount: 2, durationMs: 48000, metadata: null, video: null }));
  const libraries = vi.fn(async () => []);
  const cache = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const runtime = createWebTestRuntime({ media: { movieCards: async () => rows, showCards: async () => rows, libraries,
    artwork: { posterFor: () => null, showPosterFor: () => null, backdropFor: () => null } } }, () => cache);
  const queries = runtime.queries.catalogQueries;
  if (kind === 'movies') await cache.fetchQuery(queries.moviesView());
  else await cache.fetchQuery(queries.showsView());
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} });
  vi.spyOn(Element.prototype, 'clientWidth', 'get').mockReturnValue(1024);
  const Page = overview.Route.options.component as ComponentType;
  render(<RouterContextProvider router={router}><WebRuntimeProvider value={runtime}>
    <QueryClientProvider client={cache}><I18nProvider locale="en"><Page /></I18nProvider></QueryClientProvider>
  </WebRuntimeProvider></RouterContextProvider>);
  const links = await screen.findAllByRole('link');
  expect(links.map(el => el.getAttribute('href'))).toEqual([`/${kind}/newer`, `/${kind}/older`]);
  expect(screen.getByText(kind === 'movies' ? 'browse.count.movies' : 'browse.count.series')).toBeTruthy();
  expect(screen.queryByRole('combobox')).toBeNull();
  expect(screen.queryByText('browse.allLibraries')).toBeNull();
  expect(screen.queryByText('browse.allGenres')).toBeNull();
  expect(libraries).not.toHaveBeenCalled();
});

it('rechecks a cached grant and never loads content after deletion or revocation', async () => {
  const { movieCards, showCards } = mount(async () => [], undefined, true);
  await screen.findByText('error.libraryNotFound');
  expect(movieCards).not.toHaveBeenCalled();
  expect(showCards).not.toHaveBeenCalled();
});

it('distinguishes a permission-list failure from a missing library', async () => {
  const { movieCards, showCards } = mount(async () => { throw Error('offline'); });
  await screen.findByText('error.serverTitle');
  expect(movieCards).not.toHaveBeenCalled();
  expect(showCards).not.toHaveBeenCalled();
});

it('loads both scoped lists and links each tile to its own detail route', async () => {
  const movies = vi.fn(async (_id: string) => [{ id: 'film', title: 'Film', addedAt: '2025-01-01' }]);
  const { showCards } = mount(async () => [grant], movies);
  await screen.findByText(grant.name);
  const links = await screen.findAllByRole('link');
  expect(links.map((link) => link.getAttribute('href'))).toEqual(['/shows/series', '/movies/film']);
  expect(movies).toHaveBeenCalledExactlyOnceWith(id);
  expect(showCards).toHaveBeenCalledExactlyOnceWith(id);
});

it('renders an empty movie library without requesting shows', async () => {
  const { movieCards, showCards } = mount(async () => [{ ...grant, kind: 'movies' }]);
  await screen.findByText('browse.otherEmpty');
  expect(movieCards).toHaveBeenCalledExactlyOnceWith(id);
  expect(showCards).not.toHaveBeenCalled();
});

it.each(['photos', 'other'] as const)('requests only the scoped %s endpoint', async (kind) => {
  const urls: URL[] = [];
  vi.stubGlobal('fetch', async (input: RequestInfo | URL) => {
    urls.push(new URL(input instanceof Request ? input.url : String(input)));
    return Response.json([]);
  });
  const { movieCards, showCards } = mount(async () => [{ ...grant, kind }]);
  await screen.findByText('browse.otherEmpty');
  expect(urls).toHaveLength(1);
  expect(urls[0]?.pathname).toMatch(new RegExp(`/${kind}$`));
  expect(urls[0]?.searchParams.get('library')).toBe(id);
  expect(movieCards).not.toHaveBeenCalled();
  expect(showCards).not.toHaveBeenCalled();
});

it('opens the specific photo linked from a shelf, but never a photo outside the scoped list', async () => {
  vi.stubGlobal('fetch', async () => Response.json([{ id: 'frame', kind: 'photo', title: 'Family trip', year: null, video: null,
    library: id, addedAt: '2026-01-02', showId: null, showTitle: null, season: null, episode: null, episodeEnd: null,
    episodeTitle: null, files: [], container: 'jpg', durationMs: null, audio: null, audioTracks: [], subtitles: [], relPath: null }]));
  const list = async () => [{...grant, kind: 'photos'}];
  mount(list, undefined, false, 'frame');
  expect(await screen.findByRole('dialog', {name:'Family trip'})).toBeTruthy();
  cleanup();
  mount(list, undefined, false, 'not-in-this-library');
  await screen.findByRole('button', { name: /Family trip/ });
  expect(screen.queryByRole('dialog')).toBeNull();
});

it('retries a scoped failure without widening the request to every library', async () => {
  const movies = vi.fn<(_id: string) => Promise<unknown[]>>()
    .mockRejectedValueOnce(Error('offline')).mockResolvedValueOnce([]);
  mount(async () => [{...grant, kind: 'movies'}], movies);
  await screen.findByText('error.serverTitle');
  fireEvent.click(screen.getByRole('button', { name: 'error.retry' }));
  await screen.findByText('browse.otherEmpty');
  expect(movies.mock.calls).toEqual([[id], [id]]);
});
