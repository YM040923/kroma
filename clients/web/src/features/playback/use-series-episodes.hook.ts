// Everything the player's episode drawer needs: the show's seasons (already in
// the catalogue cache when the watch page loaded) joined with the viewer's
// progress and watched flags. A film asks for none of it.

import { ShowId, type SubjectId } from '@kromatv/client/media';
import { EPISODE_ART_W, type EpisodesData } from '@kromatv/ui';
import { useQuery } from '@tanstack/react-query';
import { useMemo } from 'react';
import type { MovieView } from '#web/shared/lib/api';
import { useAuth } from '#web/shared/lib/auth';
import { useWatched } from '#web/shared/lib/watched';
import { useWebRuntime } from '#web/shared/lib/web-runtime';
import { progressPercent, toEpisodesData } from './series-episodes';

// A key for a query that is switched off; never sent.
const NO_SHOW = ShowId.parse('none');

export function useSeriesEpisodes(item: MovieView): EpisodesData | null {
  const webRuntime = useWebRuntime();

  const { user } = useAuth();
  const { isWatched } = useWatched();
  const showId = item.kind === 'episode' ? item.showId : null;

  const show = useQuery({
    ...webRuntime.client.query.media.show(showId ?? NO_SHOW),
    enabled: showId != null,
  });
  const progress = useQuery({
    ...webRuntime.queries.userQueries.progress(),
    enabled: user != null && showId != null,
    select: progressPercent,
  });

  return useMemo(() => {
    if (!show.data || showId == null) {
      return null;
    }
    return toEpisodesData({
      detail: show.data,
      currentId: item.id,
      watched: (id: string) => isWatched(id as SubjectId),
      progress: progress.data ?? {},
      art: (episode) =>
        webRuntime.client.media.artwork.backdropFor(episode, EPISODE_ART_W) ??
        webRuntime.client.media.artwork.posterFor(episode, EPISODE_ART_W),
    });
  }, [show.data, showId, item.id, isWatched, progress.data, webRuntime.client]);
}
