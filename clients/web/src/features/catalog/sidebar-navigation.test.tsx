// @vitest-environment jsdom
import { Library } from '@kromatv/client/media';
import { I18nProvider } from '@kromatv/ui';
import { KitProvider } from '@kromatv/ui/kit';
import { createInputTestHost, declared } from '@kromatv/ui/testing';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { act, cleanup, fireEvent, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { SidebarLayoutCard } from '#web/features/accounts/account/sidebar-layout-card.component';
import { WebRuntimeProvider } from '#web/shared/lib/web-runtime';
import { createWebTestRuntime } from '#web/shared/lib/web-runtime.fixture';
import { libraryPreferenceId } from './library-identity';
import { Sidebar, MobileTopbar } from './sidebar.component';

const session = vi.hoisted(() => ({ id: 'a' }));
vi.mock('#web/shared/lib/auth', () => ({ useAuth: () => ({ ready: true, user: { id: session.id, username: session.id, permissions: [] }, client: {} }) }));
vi.mock('#web/modules/module-host-provider.component', () => ({ useModuleNav: () => [] }));
vi.mock('#web/features/catalog/sidebar-version.component', () => ({ SidebarVersion: () => null }));
vi.mock('#web/features/catalog/server-update-notice.component', () => ({ ServerUpdateNotice: () => null, ServerUpdateIndicator: () => null }));

const cinema = Library.parse({ id: '电影 %', name: '动漫电影', kind: 'movies', path: '', itemCount: 8 });
const series = Library.parse({ ...cinema, id: 'domestic', name: '国漫', kind: 'shows' });
const host = createInputTestHost();
const click = (el: HTMLElement) => { host.clearPressGuard(); fireEvent.click(el); };
const changes = (name: string) => screen.getByRole('switch', { name: `Show ${name} in the sidebar` });
beforeEach(() => { session.id = 'a'; vi.stubGlobal('fetch', vi.fn().mockRejectedValue(Error('Unexpected external request'))); });
afterEach(() => { cleanup(); host.clearPressGuard(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

function mount(width = 1440) {
  const cache = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const prefs = new Map<string,string[]>();
  const account = {
    sidebarLayout: vi.fn(async () => ({ hidden: prefs.get(session.id) ?? [] })),
    setSidebarLayout: vi.fn(async ({ hidden }: { hidden: string[] }) => { prefs.set(session.id, hidden); return { hidden }; }),
  };
  const runtime = createWebTestRuntime({ accounts: account, media: { libraries: async () => session.id === 'a' ? [cinema, series] : [series] } }, () => cache);
  cache.setQueryData(runtime.queries.userQueries.releases().queryKey, { unseen: null, current: '1', releases: [] });
  const root = createRootRoute({ component: () => <><Sidebar /><MobileTopbar /><Outlet /></> });
  const edit = createRoute({ getParentRoute: () => root, path: '/account', component: () => <SidebarLayoutCard key={session.id} viewer={session.id} /> });
  const detail = createRoute({ getParentRoute: () => root, path: '/libraries/$id', component: () => <p>Opened library</p> });
  const router = createRouter({ routeTree: root.addChildren([edit, detail]), history: createMemoryHistory({ initialEntries: ['/account'] }) });
  const tree = (user: string) => <WebRuntimeProvider value={runtime}><QueryClientProvider client={cache}>
    <KitProvider config={{ formFactor: 'browser', designWidth: width }}><I18nProvider locale="en"><RouterProvider key={user} router={router} /></I18nProvider></KitProvider>
  </QueryClientProvider></WebRuntimeProvider>;
  const view = host.render(tree(session.id));
  return { cache, runtime, account, router, changeUser: (id: string) => { session.id = id; view.rerender(tree(id)); } };
}

it('saves sidebar choices separately from home rows, keeps them through a library rename, and restores all links', async () => {
  const { cache, runtime, account } = mount();
  const homeKey = runtime.queries.userQueries.homeLayout('a').queryKey;
  const home = { order: ['lib:domestic', 'hero'], hidden: ['continue'] };
  cache.setQueryData(homeKey, home);
  await screen.findByRole('link', { name: cinema.name });
  click(changes(series.name));
  click(changes('Photos'));
  click(screen.getByRole('button',{name:'Save'}));
  await waitFor(() => expect(screen.queryByRole('link',{name:series.name})).toBeNull());
  expect(account.setSidebarLayout).toHaveBeenCalledWith({hidden:[libraryPreferenceId(series.id),'nav.photos']});
  expect(screen.queryByRole('link',{name:'Photos'})).toBeNull();
  expect(cache.getQueryData(homeKey)).toEqual(home);
  const updated = '国漫精选';
  act(() => { cache.setQueryData(runtime.queries.catalogQueries.libraries('a').queryKey, [cinema, {...series,name:updated}]); });
  expect((await screen.findByRole('switch', {name:`Show ${updated} in the sidebar`})).getAttribute('aria-checked')).toBe('false');
  expect(screen.queryByRole('link',{name:updated})).toBeNull();
  click(screen.getByRole('button',{name:'Restore defaults'}));
  click(screen.getByRole('button',{name:'Save'}));
  expect(await screen.findByRole('link',{name:updated})).toBeTruthy();
  expect(account.setSidebarLayout).toHaveBeenLastCalledWith({hidden:[]});
});

it('keeps the actual library ID in the link, highlights only that library, and leaves the rail scrollable', async () => {
  const { router } = mount();
  const film = await screen.findByRole('link', { name: cinema.name });
  expect(decodeURIComponent(film.getAttribute('href')!)).toBe(`/libraries/${cinema.id}`);
  expect(screen.getByRole('link',{name:'Movies'})).toBeTruthy();
  expect(screen.getByRole('link',{name:'Series'})).toBeTruthy();
  const ancestors = []; for (let node: HTMLElement | null = film; node; node = node.parentElement) ancestors.push(node);
  expect(ancestors.some(el => declared(el, 'scrollbar-width') === 'none' && ['auto','scroll'].includes(getComputedStyle(el).overflowY))).toBe(true);
  click(film);
  await screen.findByText('Opened library');
  expect(router.state.matches.at(-1)?.params).toMatchObject({ id: cinema.id });
  expect(screen.getByRole('link',{name:cinema.name}).getAttribute('aria-current')).toBe('page');
  expect(screen.getByRole('link',{name:series.name}).getAttribute('aria-current')).not.toBe('page');
});

it('loads the next account’s links and never gives a regular viewer management entries', async () => {
  const { changeUser } = mount();
  await screen.findByRole('link',{name:cinema.name});
  expect(screen.queryByRole('link',{name:'Server'})).toBeNull();
  expect(screen.queryByRole('switch',{name:'Show Server in the sidebar'})).toBeNull();
  changeUser('b');
  await screen.findByRole('switch',{name:`Show ${series.name} in the sidebar`});
  expect(screen.queryByRole('link',{name:cinema.name})).toBeNull();
  expect(screen.queryByRole('switch',{name:`Show ${cinema.name} in the sidebar`})).toBeNull();
});

it('keeps a route back to settings when every optional link is disabled', async () => {
  const { cache, runtime } = mount();
  await screen.findByRole('link', { name: cinema.name });
  const hidden = ['nav.home','nav.search','nav.films','nav.series','nav.history','nav.photos','nav.other','nav.genres','nav.myList','nav.connectDevice','releases.title',libraryPreferenceId(cinema.id),libraryPreferenceId(series.id)];
  act(() => { cache.setQueryData(runtime.queries.userQueries.sidebarLayout('a').queryKey, { hidden }); });
  await waitFor(() => expect(screen.queryByRole('link', { name: cinema.name })).toBeNull());
  expect(screen.getByTitle('Account')).toBeTruthy();
  expect(screen.queryByText('More', {selector:'aside *'})).toBeNull();
  click(screen.getByTitle('Account'));
  expect(await screen.findByText('Account settings')).toBeTruthy();
});

it('uses the same library links in the phone drawer and closes it on selection', async () => {
  const { router } = mount(390);
  const menu = await screen.findByRole('button',{name:'Menu'});
  expect(screen.queryByRole('link',{name:series.name})).toBeNull();
  click(menu);
  const row = await screen.findByRole('link',{name:series.name});
  click(row);
  await screen.findByText('Opened library');
  expect(router.state.matches.at(-1)?.params).toMatchObject({ id: series.id });
  await waitFor(() => expect(screen.queryByRole('link',{name:series.name})).toBeNull());
});
