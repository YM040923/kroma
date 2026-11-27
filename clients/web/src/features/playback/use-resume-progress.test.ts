// @vitest-environment jsdom
import { UserId } from '@kromatv/client/accounts';
import { act, cleanup, renderHook, waitFor } from '@testing-library/react';
import { createElement, type ReactNode } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { useResumeAnchor } from '#web/features/playback/use-resume-anchor.hook';
import { movie } from '#web/features/playback/use-video-playback.fixture';
import { WebRuntimeProvider } from '#web/shared/lib/web-runtime';
import { createWebTestRuntime } from '#web/shared/lib/web-runtime.fixture';

const itemProgress = vi.fn();
const item = movie();
const first = UserId.parse('u1');
function setup() {
  const runtime = createWebTestRuntime({ playback: { itemProgress } });
  const open = () => runtime.queryClient.prefetchQuery(runtime.queries.catalogQueries.resumeProgress(item.id, first));
  const render = () => renderHook(({ viewer }) => useResumeAnchor(item, viewer), {
    initialProps: { viewer: first },
    wrapper: ({ children }: { children: ReactNode }) => createElement(WebRuntimeProvider, { value: runtime }, children),
  });
  return { open, render };
}
afterEach(() => { cleanup(); itemProgress.mockReset(); });

describe('resume snapshot per opening', () => {
  it('shares the loader read and never reapplies it over a seek or session refresh', async () => {
    const { open, render } = setup();
    itemProgress.mockResolvedValue({ positionMs: 40_000 });
    await open();
    const view = render();
    expect(view.result.current.bootAnchor).toBe(40);
    act(() => view.result.current.setAnchor(65));
    view.rerender({ viewer: first });
    expect(view.result.current.anchor).toBe(65);
    expect(view.result.current.bootAnchor).toBe(40);
    expect(itemProgress).toHaveBeenCalledTimes(1);
    itemProgress.mockResolvedValue({ positionMs: 52_000 });
    view.rerender({ viewer: UserId.parse('u2') });
    await waitFor(() => expect(view.result.current.anchor).toBe(52));
    expect(itemProgress).toHaveBeenCalledTimes(2);
  });

  it.each([false, true])('waits for the new read on immediate reopen (failure: %s)', async (fails) => {
    const { open, render } = setup();
    itemProgress.mockResolvedValue({ positionMs: 40_000 });
    await open();
    const previous = render();
    expect(previous.result.current.anchor).toBe(40);
    previous.unmount();
    let finish!: () => void;
    itemProgress.mockImplementationOnce(() => new Promise((resolve, reject) => {
      finish = () => fails ? reject(new Error('offline')) : resolve({ positionMs: 72_000 });
    }));
    const pending = open();
    const next = render();
    expect(next.result.current.bootAnchor).toBeNull();
    await act(async () => { finish(); await pending; });
    await waitFor(() => expect(next.result.current.bootAnchor).toBe(fails ? 0 : 72));
    expect(itemProgress).toHaveBeenCalledTimes(2);
  });
});
