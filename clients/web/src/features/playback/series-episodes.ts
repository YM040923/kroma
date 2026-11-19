// Joins the catalogue's show detail with the viewer's progress / watched flags
// into the drawer's own shape. Kept free of React and of the client instance so
// the ordering and progress rules are testable on their own.

import type { MediaItem, ProgressEntry, ShowDetail } from '@kromatv/client/media';
import { formatRuntime } from '@kromatv/core';
import type { EpisodesData, SeasonItem } from '@kromatv/ui';

export interface SeriesEpisodesInput {
  detail: ShowDetail;
  currentId: string;
  watched: (id: string) => boolean;
  progress: Record<string, number>;
  art: (episode: MediaItem) => string | null;
}

/** Resume position as a rounded percentage, keyed by item id. Entries with no
 *  duration or no position are left out rather than drawn as 0 %. */
export function progressPercent(entries: readonly ProgressEntry[]): Record<string, number> {
  const map: Record<string, number> = {};
  for (const entry of entries) {
    const duration = entry.durationMs ?? 0;
    if (duration > 0 && entry.positionMs > 0) {
      map[entry.itemId] = Math.min(100, Math.round((entry.positionMs / duration) * 100));
    }
  }
  return map;
}

export function toEpisodesData(input: SeriesEpisodesInput): EpisodesData {
  const seasons: SeasonItem[] = input.detail.seasons
    .slice()
    .sort((a, b) => a.number - b.number)
    .map((season) => ({
      number: season.number,
      episodes: season.episodes
        .filter((episode) => episode.episode != null)
        .slice()
        .sort((a, b) => (a.episode ?? 0) - (b.episode ?? 0))
        .map((episode) => ({
          id: episode.id,
          season: episode.season ?? season.number,
          episode: episode.episode ?? 0,
          title: episode.episodeTitle ?? episode.title,
          subtitle:
            episode.durationMs != null && episode.durationMs > 0
              ? formatRuntime(episode.durationMs)
              : undefined,
          artUrl: input.art(episode),
          progress: input.progress[episode.id] ?? null,
          watched: input.watched(episode.id),
        })),
    }))
    .filter((season) => season.episodes.length > 0);
  return {
    showTitle: input.detail.show.title,
    currentId: input.currentId,
    seasons,
  };
}
