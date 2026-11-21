import type { EpisodeRef } from '@kromatv/client/requests';
import { useT } from '@kromatv/ui';
import { useState } from 'react';
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

  const doRequest = (seasons: number[] | null, episodes?: EpisodeRef[]) => {
    const { tmdbId, kind, title, year } = view;
    if (tmdbId == null) {
      return;
    }
    setBusy(true);
    setError(null);
    void acquire({ kind, tmdbId, title, year }, { seasons, episodes })
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
      doRequest(null);
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
    toggleEpisode,
    requestSelected,
    requestSeason,
    requestAllSeasons,
    clearSelection: () => setSelected(new Set()),
    onRequestClick,
  };
}
