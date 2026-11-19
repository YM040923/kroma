// @vitest-environment jsdom

import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

type HookModule = typeof import('./sidebar-collapsed.hook');

const KEY = 'kroma:sidebar-mode';
const LEGACY_KEY = 'kroma:sidebar-collapsed';

// The module keeps the shared mode in memory between mounts, which is the
// feature under test; a fresh import starts from the stored value again.
async function hookModule(): Promise<HookModule> {
  vi.resetModules();
  return import('./sidebar-collapsed.hook');
}

beforeEach(() => {
  window.localStorage.clear();
});

afterEach(() => {
  cleanup();
  window.localStorage.clear();
});

describe('useSidebarMode', () => {
  it('starts expanded when nothing is stored', async () => {
    const { useSidebarMode } = await hookModule();
    const { result } = renderHook(() => useSidebarMode());

    expect(result.current[0]).toBe('expanded');
  });

  it('migrates the old folded boolean to the rail', async () => {
    window.localStorage.setItem(LEGACY_KEY, '1');
    const { useSidebarMode } = await hookModule();
    const { result } = renderHook(() => useSidebarMode());

    expect(result.current[0]).toBe('rail');
    expect(window.localStorage.getItem(KEY)).toBe('rail');
    expect(window.localStorage.getItem(LEGACY_KEY)).toBeNull();
  });

  it('migrates the old unfolded boolean to expanded', async () => {
    window.localStorage.setItem(LEGACY_KEY, '0');
    const { useSidebarMode } = await hookModule();
    const { result } = renderHook(() => useSidebarMode());

    expect(result.current[0]).toBe('expanded');
    expect(window.localStorage.getItem(KEY)).toBe('expanded');
    expect(window.localStorage.getItem(LEGACY_KEY)).toBeNull();
  });

  it('restores a stored three-state mode', async () => {
    window.localStorage.setItem(KEY, 'hidden');
    const { useSidebarMode } = await hookModule();
    const { result } = renderHook(() => useSidebarMode());

    expect(result.current[0]).toBe('hidden');
  });

  it('persists the mode it is given', async () => {
    const { useSidebarMode } = await hookModule();
    const { result } = renderHook(() => useSidebarMode());

    act(() => result.current[1]('rail'));

    expect(result.current[0]).toBe('rail');
    expect(window.localStorage.getItem(KEY)).toBe('rail');
  });

  it('shares one mode between the catalogue and the console sidebars', async () => {
    const { useSidebarMode } = await hookModule();
    const catalogue = renderHook(() => useSidebarMode());
    const admin = renderHook(() => useSidebarMode());

    act(() => catalogue.result.current[1]('hidden'));

    expect(catalogue.result.current[0]).toBe('hidden');
    expect(admin.result.current[0]).toBe('hidden');
  });

  it('ignores a stored value that is not a mode', async () => {
    window.localStorage.setItem(KEY, 'collapsed');
    const { useSidebarMode } = await hookModule();
    const { result } = renderHook(() => useSidebarMode());

    expect(result.current[0]).toBe('expanded');
  });
});
