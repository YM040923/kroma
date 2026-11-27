// @vitest-environment jsdom

import { CardSection } from '@kromatv/client/media';
import { createInputTestHost, onScreen } from '@kromatv/ui/testing';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, cleanup, fireEvent, screen } from '@testing-library/react';
import { type ReactElement, useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { HomeLayout } from '#web/features/catalog/home/home-layout';
import * as layoutApi from '#web/features/catalog/home/home-layout-api';
import { Route as AccountRoute } from '#web/routes/_app.account';
import { WebRuntimeProvider } from '#web/shared/lib/web-runtime';
import { createWebTestRuntime } from '#web/shared/lib/web-runtime.fixture';
import { AccountPage } from './account-page.component';
import type { AccountSection } from './account-sections';
import { HomeLayoutCard } from './home-layout-card.component';

vi.mock('#web/shared/lib/auth', () => ({
  useAuth: () => ({ user: { id: 'viewer-a', username: 'Ada', email: 'ada@kroma.test' } }),
}));

const { render, clearPressGuard } = createInputTestHost();
const VIEWER = 'viewer-a';
const INITIAL: HomeLayout = {
  order: ['festival', 'absent-child', 'staff'],
  hidden: ['absent-child', 'hidden-only'],
};

function section(id: string, title: string) {
  return CardSection.parse({
    id,
    title,
    layout: 'rail',
    notes: {},
    items: [
      {
        type: 'show',
        show: {
          id: 's1',
          title: 'Dark',
          year: null,
          library: 'l1',
          video: null,
          addedAt: '2026-01-01T00:00:00Z',
          seasonCount: 1,
          episodeCount: 1,
        },
      },
    ],
  });
}

const sections = [section('festival', 'Festival picks'), section('staff', 'Staff favourites')];

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

function mount({
  load = async () => INITIAL,
  persist,
  content = (viewer) => <HomeLayoutCard key={viewer} viewer={viewer} />,
}: {
  load?: (viewer: string) => Promise<HomeLayout>;
  persist?: () => Promise<void>;
  content?: (viewer: string) => ReactElement;
} = {}) {
  // No API traffic: exercise real query observers, including the cache update promised by the writer.
  vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('Unexpected network request')));
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: Infinity, refetchOnMount: false } },
  });
  const runtime = createWebTestRuntime({}, () => cache);
  const q = runtime.queries.userQueries;
  const options = q.homeLayout;
  vi.spyOn(q, 'homeLayout').mockImplementation((viewer) => ({
    ...options(viewer),
    queryFn: () => load(viewer),
    retry: false,
  }));
  for (const viewer of [VIEWER, 'viewer-b']) {
    cache.setQueryData(runtime.queries.catalogQueries.libraries(viewer).queryKey, []);
  }
  cache.setQueryData(q.home().queryKey, sections);
  cache.setQueryData(q.continueWatching().queryKey, []);
  const write = vi.spyOn(layoutApi, 'saveAccountHomeLayout').mockImplementation(
    async (rt, viewer, next) => {
      await persist?.();
      rt.queryClient.setQueryData(q.homeLayout(viewer).queryKey, next);
    },
  );
  const tree = (viewer: string) => (
    <QueryClientProvider client={cache}>
      <WebRuntimeProvider value={runtime}>
        {onScreen(content(viewer))}
      </WebRuntimeProvider>
    </QueryClientProvider>
  );
  const { rerender } = render(tree(VIEWER));
  return { write, cache, q, switchViewer: (viewer: string) => rerender(tree(viewer)) };
}

const button = (name: string) => screen.getByRole('button', { name });
const toggle = (title: string) => screen.getByRole('switch', { name: `Show ${title}` });
const ready = () => screen.findByRole('switch', { name: 'Show Festival picks' });

function click(el: HTMLElement) {
  clearPressGuard();
  fireEvent.click(el);
}

afterEach(() => {
  cleanup();
  clearPressGuard();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function AccountTabsFixture() {
  const [section, setSection] = useState<AccountSection>('profile');
  return <AccountPage section={section} onSection={setSection} />;
}

describe('account home sections', () => {
  it('accepts the home deep link and opens the editor from a peer account tab', async () => {
    const search = new URL('https://kroma.test/account?section=home').searchParams;
    const validate = AccountRoute.options.validateSearch;
    const parsed = typeof validate === 'function' ? validate(Object.fromEntries(search)) : undefined;
    expect(parsed).toEqual({ section: 'home' });
    const load = vi.fn<(viewer: string) => Promise<HomeLayout>>().mockResolvedValue(INITIAL);
    const scroll = HTMLElement.prototype.scrollIntoView;
    HTMLElement.prototype.scrollIntoView = vi.fn(); // jsdom has no scrolling layout.
    try {
      mount({ load, content: () => <AccountTabsFixture /> });
      const tab = screen.getByRole('radio', { name: 'Home sections' });
      expect(load).not.toHaveBeenCalled();
      click(tab);
      await ready();
      expect(tab.getAttribute('aria-checked')).toBe('true');
      expect(load).toHaveBeenCalledWith(VIEWER);
    } finally {
      HTMLElement.prototype.scrollIntoView = scroll;
    }
  });

  it('has no editable defaults during a slow or failed layout read; retry loads this viewer', async () => {
    const first = deferred<HomeLayout>();
    const load = vi.fn<(viewer: string) => Promise<HomeLayout>>()
      .mockReturnValueOnce(first.promise)
      .mockResolvedValue({ order: [], hidden: ['festival'] });
    const { write } = mount({ load });

    expect(screen.getByLabelText('Loading…')).toBeTruthy();
    expect(screen.queryByRole('switch')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Save' })).toBeNull();
    first.reject(new Error('offline'));
    expect(await screen.findByText('Could not load home settings.')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Restore defaults' })).toBeNull();
    click(button('Try again'));

    expect((await ready()).getAttribute('aria-checked')).toBe('false');
    expect(load.mock.calls).toEqual([[VIEWER], [VIEWER]]);
    expect(button('Save').getAttribute('aria-disabled')).toBe('true');
    expect(write).not.toHaveBeenCalled();
  });

  it('moves a hidden row by keyboard and keeps absent rows in their slots when saving', async () => {
    const { write, cache, q } = mount();
    click(await ready());
    expect(toggle('Festival picks').getAttribute('aria-checked')).toBe('false');
    expect(button('Move Festival picks up').getAttribute('aria-disabled')).toBe('true');
    expect(screen.getAllByText('Nothing to show yet').length).toBeGreaterThan(0);
    const down = button('Move Festival picks down');
    clearPressGuard();
    act(() => down.focus());
    fireEvent.keyDown(down, { key: 'Enter', code: 'Enter' });
    fireEvent.keyUp(down, { key: 'Enter', code: 'Enter' });
    expect(screen.getAllByRole('switch')[0]).toBe(toggle('Staff favourites'));
    expect(write).not.toHaveBeenCalled();

    click(button('Save'));
    await screen.findByText('Saved');
    expect(write).toHaveBeenCalledTimes(1);
    const [, viewer, saved] = write.mock.calls[0]!;
    expect(viewer).toBe(VIEWER);
    expect(saved.order.slice(0, 3)).toEqual(['staff', 'absent-child', 'festival']);
    expect(saved.hidden).toEqual(['absent-child', 'hidden-only', 'festival']);

    act(() => { cache.setQueryData(q.home().queryKey, [...sections, section('absent-child', 'Back today')]); });
    const back = await screen.findByRole('switch', { name: 'Show Back today' });
    expect(back.getAttribute('aria-checked')).toBe('false');
    expect(screen.getAllByRole('switch').slice(0, 3)).toEqual([
      toggle('Staff favourites'), back, toggle('Festival picks'),
    ]);
  });

  it('retries a failed reset with two empty arrays, not an expanded default inventory', async () => {
    const persist = vi.fn<() => Promise<void>>()
      .mockRejectedValueOnce(new Error('not saved'))
      .mockResolvedValue(undefined);
    const { write } = mount({ persist });
    await ready();
    click(button('Restore defaults'));
    expect(write).not.toHaveBeenCalled();
    click(button('Save'));
    await screen.findByText("Couldn't save");
    click(button('Try again'));
    await screen.findByText('Saved');

    expect(write.mock.calls.map(([, viewer, layout]) => [viewer, layout])).toEqual([
      [VIEWER, { order: [], hidden: [] }],
      [VIEWER, { order: [], hidden: [] }],
    ]);
    expect(button('Save').getAttribute('aria-disabled')).toBe('true');
  });

  it('does not overlap writes or replace edits made while the first save is pending', async () => {
    const first = deferred<void>();
    const persist = vi.fn<() => Promise<void>>().mockReturnValueOnce(first.promise).mockResolvedValue(undefined);
    const { write } = mount({ persist });
    click(await ready());
    const submit = button('Save');
    click(submit);
    click(submit);
    await screen.findByRole('button', { name: 'Saving…' });
    click(toggle('Staff favourites'));
    click(button('Move Staff favourites up'));
    expect(write).toHaveBeenCalledTimes(1);
    first.resolve();

    click(await screen.findByRole('button', { name: 'Save' }));
    await screen.findByText('Saved');
    expect(write).toHaveBeenCalledTimes(2);
    const [firstWrite, secondWrite] = write.mock.calls.map(([, , layout]) => layout);
    expect(firstWrite).toEqual({ order: INITIAL.order, hidden: [...INITIAL.hidden, 'festival'] });
    expect(secondWrite?.order.slice(0, 3)).toEqual(['staff', 'absent-child', 'festival']);
    expect(secondWrite?.hidden).toEqual([...INITIAL.hidden, 'festival', 'staff']);
  });

  it('does not carry a pending save or its completion into the next account', async () => {
    const first = deferred<void>();
    const load = async (id: string) => id === VIEWER ? INITIAL : { order: [], hidden: ['staff'] };
    const { write, cache, q, switchViewer } = mount({ load, persist: () => first.promise });
    click(await ready());
    click(button('Save'));
    await screen.findByRole('button', { name: 'Saving…' });
    switchViewer('viewer-b');
    expect((await ready()).getAttribute('aria-checked')).toBe('true');
    expect(toggle('Staff favourites').getAttribute('aria-checked')).toBe('false');
    await act(async () => { first.resolve(); await first.promise; });
    expect(cache.getQueryData(q.homeLayout('viewer-b').queryKey)).toEqual({ order: [], hidden: ['staff'] });
    expect(write.mock.calls.map(([, viewer]) => viewer)).toEqual([VIEWER]);
    expect(screen.queryByText('Saved')).toBeNull();
    expect(button('Save').getAttribute('aria-disabled')).toBe('true');
  });
});
