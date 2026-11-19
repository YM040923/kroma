import type { Query, RequestContext } from '../../core/client';
import {
  type DiscoverAvailability,
  DiscoverDetail,
  type DiscoverFilterSort,
  DiscoverFilterResponse,
  DiscoverResponse,
  DiscoverSeasonEpisodes,
  type DiscoverType,
  LocalDiscoverResponse,
  type TmdbKind,
} from './schemas';

/** What a discovery listing asks for. `all` is the absence of a type filter and
 * page one is the absence of a page, which is how the server reads them. */
export interface DiscoverPage {
  type?: DiscoverType;
  page?: number;
  /** Keep only titles this library does (not) hold. Searches honour it too. */
  state?: DiscoverAvailability;
}

/** One filtered TMDB browse query (`type` is required: TMDB splits movie/tv). */
export interface DiscoverFilterOptions {
  type: TmdbKind;
  sort?: DiscoverFilterSort;
  /** ISO 639-1 original-language filter. */
  language?: string;
  year?: number;
  /** First year of a decade, e.g. 1990. */
  decade?: number;
  /** TMDB genre id. */
  genre?: number;
  page?: number;
  state?: DiscoverAvailability;
}

/** One local-catalogue discovery query. `type` absent/`all` keeps both kinds;
 * `watched` absent/`any` keeps watched and unwatched alike. */
export interface LocalDiscoverOptions {
  q?: string;
  type?: DiscoverType;
  library?: string;
  year?: number;
  decade?: number;
  /** TMDB genre id, matched against the stored genre ids. */
  genre?: number;
  /** Minimum provider rating (0-10). */
  rating?: number;
  watched?: 'any' | 'watched' | 'unwatched';
  page?: number;
  perPage?: number;
}

const NARROWED: Readonly<Record<DiscoverType, string | undefined>> = {
  movie: 'movie',
  tv: 'tv',
  all: undefined,
};

const beyondFirst = (page = 1) => (page > 1 ? page : undefined);
const narrowedState = (state?: DiscoverAvailability) =>
  state && state !== 'any' ? state : undefined;

export function discoverQuery({ type = 'all', page, state }: DiscoverPage = {}): Query {
  return {
    type: NARROWED[type],
    page: beyondFirst(page),
    state: narrowedState(state),
  };
}

/** `GET /api/discover/filter`'s query: only the values that narrow it. */
export function discoverFilterQuery(opts: DiscoverFilterOptions): Query {
  return {
    type: opts.type,
    sort: opts.sort && opts.sort !== 'popularity' ? opts.sort : undefined,
    language: opts.language,
    year: opts.year,
    decade: opts.decade,
    genre: opts.genre,
    page: beyondFirst(opts.page),
    state: narrowedState(opts.state),
  };
}

/** `GET /api/discover/local`'s query: only the values that narrow it. */
export function localDiscoverQuery(opts: LocalDiscoverOptions): Query {
  return {
    q: opts.q?.trim() || undefined,
    type: opts.type && opts.type !== 'all' ? opts.type : undefined,
    library: opts.library,
    year: opts.year,
    decade: opts.decade,
    genre: opts.genre,
    rating: opts.rating,
    watched: opts.watched && opts.watched !== 'any' ? opts.watched : undefined,
    page: beyondFirst(opts.page),
    perPage: opts.perPage,
  };
}

/** TMDB discovery for the request flow: search titles the library may not have,
 * trending for the empty state, and a title detail with its season list. */
export default function discoveryApi(ctx: RequestContext) {
  return {
    search: (query: string, opts?: DiscoverPage) =>
      ctx.get('/discover/search', DiscoverResponse, {
        query: { q: query, ...discoverQuery(opts) },
      }),

    trending: (opts?: DiscoverPage) =>
      ctx.get('/discover/trending', DiscoverResponse, {
        query: discoverQuery(opts),
        concurrency: 'share',
      }),

    /** TMDB's browse endpoint under the same local availability flags as search:
     * sort, original language, year/decade, genre, pagination and in-library
     * state. */
    filter: (opts: DiscoverFilterOptions) =>
      ctx.get('/discover/filter', DiscoverFilterResponse, {
        query: discoverFilterQuery(opts),
        concurrency: 'latest',
      }),

    /** The local catalogue as a filterable discovery grid, filtered in the
     * database (never the whole library shipped to the client to slice). */
    local: (opts: LocalDiscoverOptions = {}) =>
      ctx.get('/discover/local', LocalDiscoverResponse, {
        query: localDiscoverQuery(opts),
        concurrency: 'latest',
      }),

    /** One title's request-flow detail. `kind` follows the route vocabulary
     * (`movie` | `tv`); the response speaks the catalog's (`movie` | `show`). */
    detail: (kind: TmdbKind, tmdbId: number) =>
      ctx.get('/discover/:kind/:tmdbId', DiscoverDetail, { params: { kind, tmdbId } }),

    season: (tmdbId: number, season: number) =>
      ctx.get('/discover/tv/:tmdbId/season/:season', DiscoverSeasonEpisodes, {
        params: { tmdbId, season },
      }),
  };
}

declare module '../../core/client' {
  interface Domains {
    discovery: ReturnType<typeof discoveryApi>;
  }
}
