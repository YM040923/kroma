import type { UserId } from '@kromatv/client/accounts';
import { useQuery } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import type { MovieView } from '#web/shared/lib/api';
import { useWebRuntime } from '#web/shared/lib/web-runtime';

export interface ResumeAnchor {
  anchor: number;
  setAnchor: React.Dispatch<React.SetStateAction<number>>;
  bootAnchor: number | null;
}

/** Resolves the stored progress into the anchor the HLS master is remuxed from.
 * `bootAnchor` stays `null` until that resolution lands. */
export function useResumeAnchor(
  item: MovieView,
  viewer: UserId | null,
): ResumeAnchor {
  const webRuntime = useWebRuntime();
  const [anchor, setAnchor] = useState(0);
  const [resolved, setResolved] = useState<{
    id: MovieView['id'];
    viewer: UserId;
    value: number;
  } | null>(null);
  const [seen, setSeen] = useState({ id: item.id, viewer });
  const progress = useQuery({
    ...webRuntime.queries.catalogQueries.resumeProgress(item.id, viewer ?? ''),
    // The loader starts a fresh read for each opening. Join it, even when
    // last time's value is still cached, without refetching on mount/focus.
    staleTime: Infinity,
    enabled: viewer !== null,
  }, webRuntime.queryClient);
  if (seen.id !== item.id || seen.viewer !== viewer) {
    setSeen({ id: item.id, viewer });
    setAnchor(0);
    setResolved(null);
  }
  const resolvedHere =
    resolved?.id === item.id && resolved.viewer === viewer ? resolved.value : null;
  const pending = progress.isPending || progress.isFetching;
  const bootAnchor = viewer ? resolvedHere : 0;
  useEffect(() => {
    if (!viewer || resolvedHere !== null || pending) {
      return;
    }
    const p = progress.data;
    const durMs = p?.durationMs ?? item.durationMs ?? 0;
    const posSec = (p?.positionMs ?? 0) / 1000;
    const resume = p && posSec > 15 && (!durMs || p.positionMs < durMs * 0.95) ? posSec : 0;
    setAnchor(resume);
    setResolved({ id: item.id, viewer, value: resume });
  }, [viewer, resolvedHere, pending, progress.data, item.id, item.durationMs]);

  return { anchor, setAnchor, bootAnchor };
}
