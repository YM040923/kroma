import { useEffect, useState } from 'react';

const KEY = 'kroma:sidebar-collapsed';

/** Whether the desktop rail is folded to its glyphs. Read after mount, so the
 * prerendered shell and the first client frame agree on the expanded rail. */
export function useSidebarCollapsed(): [boolean, (collapsed: boolean) => void] {
  const [collapsed, set] = useState(false);
  useEffect(() => {
    try {
      set(window.localStorage.getItem(KEY) === '1');
    } catch {
      /* storage unavailable */
    }
  }, []);
  const update = (next: boolean) => {
    set(next);
    try {
      window.localStorage.setItem(KEY, next ? '1' : '0');
    } catch {
      /* storage unavailable */
    }
  };
  return [collapsed, update];
}
