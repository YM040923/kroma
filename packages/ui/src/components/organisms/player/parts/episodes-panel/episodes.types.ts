/** What the episode drawer draws: every season of the show being watched and
 *  the playable episodes in it. Built by the host (the web client joins the
 *  catalogue's show detail with the viewer's progress and watched flags), so
 *  the chrome never touches a payload shape. */

export interface EpisodeItem {
  id: string;
  season: number;
  episode: number;
  title: string;
  /** Runtime / air date line, already human. */
  subtitle?: string;
  artUrl?: string | null;
  /** Resume position, 0..100. Absent when the viewer has not started it. */
  progress?: number | null;
  watched?: boolean;
}

export interface SeasonItem {
  number: number;
  episodes: EpisodeItem[];
}

export interface EpisodesData {
  /** The show's title, for the drawer's heading. */
  showTitle: string;
  /** What is playing right now. */
  currentId: string;
  seasons: SeasonItem[];
}

/** The episodes in reading order: season, then episode. Drives the previous /
 *  next keys and the ends-of-the-run checks, so both agree on what "before" and
 *  "after" mean. */
export function allEpisodes(data: EpisodesData): EpisodeItem[] {
  return data.seasons
    .slice()
    .sort((a, b) => a.number - b.number)
    .flatMap((season) => season.episodes.slice().sort((a, b) => a.episode - b.episode));
}

export function episodeAt(data: EpisodesData, offset: -1 | 1): EpisodeItem | null {
  const list = allEpisodes(data);
  const at = list.findIndex((episode) => episode.id === data.currentId);
  if (at < 0) {
    return null;
  }
  return list[at + offset] ?? null;
}

/** The season the playing episode belongs to, else the first one. */
export function currentSeason(data: EpisodesData): number {
  const hit = data.seasons.find((season) =>
    season.episodes.some((episode) => episode.id === data.currentId),
  );
  return hit?.number ?? data.seasons[0]?.number ?? 1;
}

export function hasEpisodes(data: EpisodesData | null | undefined): data is EpisodesData {
  return data != null && data.seasons.some((season) => season.episodes.length > 0);
}
