import type { EpisodeRef } from '@kromatv/client/requests';
import { useT } from '@kromatv/ui';
import { useCallback, useState } from 'react';
import { EPISODES_ANCHOR } from '#web/shared/lib/show-search';
import { epKey, toEpisodeRefs, toggle } from '#web/features/catalog/episode-selection';
import type { TitleView } from '#web/shared/lib/title-view';
import { useAcquisition } from '#web/shared/lib/use-acquisition.hook';

function nextViewAfterRequest(
  v: TitleView,
  status: TitleView['requestStatus'],
  seasons: number[] | null,
  episodes?: EpisodeRef[],
): TitleView {
  if (episodes?.length) {
    return { ...v, requestStatus: status };
  }
  const target = new Set(
    seasons ?? v.seasons.filter((s) => !s.available && !s.requested).map((s) => s.number),
  );
  return {
    ...v,
    requestStatus: status,
    seasons: v.seasons.map((s) => (target.has(s.number) ? { ...s, requested: true } : s)),
  };
}

/** The library already holds what was asked for: no request exists, so the
 *  seasons are marked held rather than wanted. */
function nextViewAfterOwned(v: TitleView, seasons: number[] | null): TitleView {
  if (v.seasons.length === 0) {
    return { ...v, requestStatus: 'available' };
  }
  const target = new Set(seasons ?? v.seasons.filter((s) => !s.available).map((s) => s.number));
  return {
    ...v,
    requestStatus: 'available',
    seasons: v.seasons.map((s) => (target.has(s.number) ? { ...s, available: true } : s)),
  };
}

/** What an accepted ask changes on screen: already held, or wanted and now in
 *  flight (a review queue reads as pending, a started acquisition as searching). */
function nextViewAfterAsk(
  v: TitleView,
  state: 'owned' | 'pending_review' | 'started',
  seasons: number[] | null,
  episodes?: EpisodeRef[],
): TitleView {
  if (state === 'owned') {
    return nextViewAfterOwned(v, seasons);
  }
  return nextViewAfterRequest(v, state === 'started' ? 'searching' : 'pending', seasons, episodes);
}

function addPendingEpisodes(prev: Set<string>, episodes: EpisodeRef[]): Set<string> {
  const next = new Set(prev);
  for (const e of episodes) {
    next.add(epKey(e.season, e.episode));
  }
  return next;
}

export interface TitleRequestState {
  view: TitleView;
  busy: boolean;
  error: string | null;
  selected: Set<string>;
  pendingEps: Set<string>;
  /** Whether the acquisition panel is open, and what it is asking for. A film's
   *  ask opens it (that is where releases are listed); a show's season buttons
   *  keep the direct path, because a show becomes a request, not a grab. */
  panelOpen: boolean;
  panelScope: { seasons: number[] | null; episodes?: EpisodeRef[] };
  openPanel: (scope?: { seasons: number[] | null; episodes?: EpisodeRef[] }) => void;
  closePanel: () => void;
  /** Take a release from the panel, or `null` for the server's own pick. */
  takeFromPanel: (candidate: Record<string, unknown> | null) => Promise<void>;
  toggleEpisode: (season: number, episode: number) => void;
  requestSelected: () => void;
  requestSeason: (season: number) => void;
  requestAllSeasons: () => void;
  clearSelection: () => void;
  onRequestClick: () => void;
}

/** Owns the acquisition half of the detail page: the locally patched `view`,
 * the episode selection, and every button that asks for the title. Every ask
 * goes through one acquisition, so a cloud-drive candidate is confirmed and
 * priced in one place whichever button started it. */
export function useTitleRequest(initial: TitleView): TitleRequestState {
  const t = useT();
  const acquire = useAcquisition();
  const [view, setView] = useState(initial);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(() => new Set());
  const [pendingEps, setPendingEps] = useState<Set<string>>(() => new Set());
  const [panelOpen, setPanelOpen] = useState(false);
  const [panelScope, setPanelScope] = useState<{
    seasons: number[] | null;
    episodes?: EpisodeRef[];
  }>({ seasons: null });

  const openPanel = useCallback((scope?: { seasons: number[] | null; episodes?: EpisodeRef[] }) => {
    setPanelScope(scope ?? { seasons: null });
    setPanelOpen(true);
  }, []);
  const closePanel = useCallback(() => setPanelOpen(false), []);

  const doRequest = (seasons: number[] | null, episodes?: EpisodeRef[], candidate?: Record<string, unknown> | null) => {
    const { tmdbId, kind, title, year } = view;
    if (tmdbId == null) {
      return;
    }
    setBusy(true);
    setError(null);
    void acquire({ kind, tmdbId, title, year }, { seasons, episodes }, { candidate })
      .then((outcome) => {
        if (outcome.state === 'failed') {
          setError(outcome.error || t('discover.requestFailed'));
          return;
        }
        if (outcome.state === 'cancelled') {
          return;
        }
        setView((v) => nextViewAfterAsk(v, outcome.state, seasons, episodes));
        if (outcome.state !== 'owned' && episodes?.length) {
          setPendingEps((prev) => addPendingEpisodes(prev, episodes));
        }
        setSelected(new Set());
      })
      .finally(() => setBusy(false));
  };

  /** The panel's `onTake`: a release the caller picked, or `null` for the
   *  server's own choice. The ask runs against the scope the panel opened with. */
  const takeFromPanel = useCallback(
    (candidate: Record<string, unknown> | null) => {
      doRequest(panelScope.seasons, panelScope.episodes, candidate);
      return Promise.resolve();
    },
    // `doRequest` closes over `view`; rebuilding on every view change is fine,
    // the panel re-renders with it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [panelScope, view],
  );

  const toggleEpisode = (season: number, episode: number) =>
    setSelected((prev) => toggle(prev, epKey(season, episode)));
  const requestSelected = () => {
    const episodes = toEpisodeRefs(selected);
    if (episodes.length > 0) {
      doRequest(null, episodes);
    }
  };
  const requestSeason = (season: number) => doRequest([season]);
  const requestAllSeasons = () => doRequest(null);
  const onRequestClick = () => {
    // With no season list there is no section to scroll to and nothing to pick,
    // so the button asks for the whole show rather than doing nothing at all.
    if (view.kind !== 'show' || view.seasons.length === 0) {
      openPanel({ seasons: null });
      return;
    }
    document.getElementById(EPISODES_ANCHOR)?.scrollIntoView({ behavior: 'smooth' });
  };

  return {
    view,
    busy,
    error,
    selected,
    pendingEps,
    panelOpen,
    panelScope,
    openPanel,
    closePanel,
    takeFromPanel,
    toggleEpisode,
    requestSelected,
    requestSeason,
    requestAllSeasons,
    clearSelection: () => setSelected(new Set()),
    onRequestClick,
  };
}
