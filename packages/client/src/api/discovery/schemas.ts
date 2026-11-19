import { z } from 'zod';
import { CastMember, CrewMember, SearchHit } from '../media';
import { RequestId, RequestKind, RequestStatus } from '../requests';

/** TMDB namespace filter for a discovery search. */
export const DiscoverType = z.enum(['movie', 'tv', 'all']);
export type DiscoverType = z.infer<typeof DiscoverType>;

/** The route vocabulary a TMDB detail is fetched under, which is TMDB's split
 * rather than the catalog's. */
export const TmdbKind = DiscoverType.exclude(['all']);
export type TmdbKind = z.infer<typeof TmdbKind>;

export const tmdbKindOf = (kind: RequestKind): TmdbKind => (kind === 'show' ? 'tv' : 'movie');

/** One TMDB title as the request flow sees it: what the provider knows plus
 * what this library and this account's requests already cover. */
export const DiscoverEntry = z.object({
  kind: RequestKind,
  tmdbId: z.number(),
  title: z.string(),
  year: z.number().nullable(),
  posterUrl: z.string().nullable(),
  backdropUrl: z.string().nullable(),
  overview: z.string().nullable(),
  rating: z.number().nullable(),
  inLibrary: z.boolean(),
  localId: z.string().nullable(),
  requestId: RequestId.nullable(),
  requestStatus: RequestStatus.nullable(),
  requestProgress: z.number().nullable(),
});
export type DiscoverEntry = z.infer<typeof DiscoverEntry>;

export const DiscoverSeason = z.object({
  season: z.number(),
  name: z.string().nullable(),
  episodeCount: z.number(),
  airDate: z.string().nullable(),
  available: z.boolean(),
  episodesAvailable: z.number(),
  requested: z.boolean(),
});
export type DiscoverSeason = z.infer<typeof DiscoverSeason>;

/** `GET /api/discover/{movie,tv}/:tmdbId`. */
export const DiscoverDetail = DiscoverEntry.extend({
  tagline: z.string().nullable(),
  genres: z.array(z.string()),
  tmdbGenreIds: z.array(z.number()).nullish(),
  runtimeMin: z.number().nullable(),
  seasons: z.array(DiscoverSeason),
  cast: z.array(CastMember),
  crew: z.array(CrewMember),
  similar: z.array(DiscoverEntry),
  airStatus: z.string().nullable(),
  nextAirDate: z.string().nullable(),
});
export type DiscoverDetail = z.infer<typeof DiscoverDetail>;

export const DiscoverEpisode = z
  .object({
    episode: z.number().int().nonnegative(),
    name: z.string().nullish(),
    overview: z.string().nullish(),
    stillUrl: z.string().nullable(),
    airDate: z.string().nullable(),
    runtimeMin: z.number().nullish(),
  })
  .transform(({ name, overview, ...episode }) => ({
    ...episode,
    title: name || null,
    overview: overview || null,
  }));
export type DiscoverEpisode = z.infer<typeof DiscoverEpisode>;

/** `GET /api/discover/tv/:tmdbId/season/:season`. */
export const DiscoverSeasonEpisodes = z.object({ episodes: z.array(DiscoverEpisode) });
export type DiscoverSeasonEpisodes = z.infer<typeof DiscoverSeasonEpisodes>;

/** `GET /api/discover/search` / `GET /api/discover/trending`. */
export const DiscoverResponse = z.object({
  results: z.array(DiscoverEntry),
  page: z.number(),
  totalPages: z.number(),
});
export type DiscoverResponse = z.infer<typeof DiscoverResponse>;

/** How a filtered discovery listing is ordered (`sort`). */
export const DiscoverFilterSort = z.enum(['popularity', 'release', 'rating']);
export type DiscoverFilterSort = z.infer<typeof DiscoverFilterSort>;

/** Whether a filtered listing keeps titles this library holds, or only the
 * ones it does not (`state`). `any` is the absence of the filter. */
export const DiscoverAvailability = z.enum(['any', 'in', 'out']);
export type DiscoverAvailability = z.infer<typeof DiscoverAvailability>;

/** `GET /api/discover/filter`: TMDB's browse endpoint, one page at a time,
 * flagged against the local catalogue. */
export const DiscoverFilterResponse = DiscoverResponse.extend({
  totalResults: z.number().default(0),
});
export type DiscoverFilterResponse = z.infer<typeof DiscoverFilterResponse>;

/** `GET /api/discover/local`: this server's own catalogue filtered server-side
 * (library, year/decade, genre, rating, watched state) with an optional text
 * query. `totalResults` counts every match, not just this page. */
export const LocalDiscoverResponse = z.object({
  results: z.array(SearchHit),
  page: z.number(),
  totalPages: z.number(),
  totalResults: z.number(),
});
export type LocalDiscoverResponse = z.infer<typeof LocalDiscoverResponse>;
