import { useSyncExternalStore } from 'react';

/** The desktop navigation has three states: the full sidebar, the folded rail
 *  of glyphs, and gone (a floating button opens it as a sheet, so the content
 *  keeps the whole width). */
export type SidebarMode = 'expanded' | 'rail' | 'hidden';

const KEY = 'kroma:sidebar-mode';
/** The boolean the two-state sidebar wrote before `hidden` existed. */
const LEGACY_KEY = 'kroma:sidebar-collapsed';

const MODES: ReadonlySet<string> = new Set(['expanded', 'rail', 'hidden']);

function isMode(value: string | null): value is SidebarMode {
  return value !== null && MODES.has(value);
}

// One mode for the whole client, not one per consumer: the catalogue and the
// console each mount their own sidebar, and reading storage per mount let them
// disagree until a reload. It is cached so a layout mounted later (a route
// change into /admin) starts on the reader's choice instead of flashing the
// expanded rail first; storage stays the source of truth across reloads.
const store: { mode: SidebarMode | null } = { mode: null };
const listeners = new Set<() => void>();

function readMode(): SidebarMode {
  try {
    const stored = window.localStorage.getItem(KEY);
    if (isMode(stored)) {
      return stored;
    }
    // Migrate the old boolean once: folded became the rail, the rest expanded.
    const legacy = window.localStorage.getItem(LEGACY_KEY);
    if (legacy !== null) {
      const migrated: SidebarMode = legacy === '1' ? 'rail' : 'expanded';
      window.localStorage.setItem(KEY, migrated);
      window.localStorage.removeItem(LEGACY_KEY);
      return migrated;
    }
  } catch {
    /* storage unavailable */
  }
  return 'expanded';
}

function snapshot(): SidebarMode {
  store.mode ??= readMode();
  return store.mode;
}

// The prerendered shell and the first client frame agree on the expanded rail;
// the stored mode lands once the store is read on the client.
function serverSnapshot(): SidebarMode {
  return 'expanded';
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** The desktop sidebar's mode and its setter, shared by every shell. */
export function useSidebarMode(): [SidebarMode, (mode: SidebarMode) => void] {
  const mode = useSyncExternalStore(subscribe, snapshot, serverSnapshot);
  const update = (next: SidebarMode) => {
    if (store.mode === next) {
      return;
    }
    store.mode = next;
    try {
      window.localStorage.setItem(KEY, next);
      window.localStorage.removeItem(LEGACY_KEY);
    } catch {
      /* storage unavailable */
    }
    for (const listener of listeners) {
      listener();
    }
  };
  return [mode, update];
}
