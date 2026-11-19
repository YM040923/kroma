import type { WebRuntime } from '#web/shared/lib/api';
import { useWebRuntime } from '#web/shared/lib/web-runtime';
// Debounced dual search: the local catalog (always) + TMDB discovery (gated
// on requests.create). Backed by TanStack Query (dedup + cache); latest-wins is
// handled by the query key changing per (query, type).

import { hasPermission } from '@kromatv/client/accounts';
import type { DiscoverEntry, DiscoverType } from '@kromatv/client/discovery';
import type { PersonHit, SearchHit } from '@kromatv/client/media';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import type { AvailabilityFilter, DiscoverSearch } from '#web/features/requests/discover-search';
import { useAuth } from '#web/shared/lib/auth';
import { useDebouncedValue } from '#web/shared/lib/use-debounced-value.hook';

export interface DiscoverSearchState {
  loading: boolean;
  local: SearchHit[];
  people: PersonHit[];
  discover: DiscoverEntry[];
  canDiscover: boolean;
}

const NO_LOCAL: { results: SearchHit[]; people: PersonHit[] } = { results: [], people: [] };
const NO_DISCOVER: DiscoverEntry[] = [];

const NOTHING_FOUND = { local: NO_LOCAL.results, people: NO_LOCAL.people, discover: NO_DISCOVER };

function narrowLocal(local: SearchHit[], type: DiscoverType): SearchHit[] {
  if (type === 'all') {
    return local;
  }
  return local.filter((h) => (type === 'movie' ? h.type === 'movie' : h.type !== 'movie'));
}

interface SearchScope {
  client: WebRuntime['client'];
  q: string;
  type: DiscoverType;
  canDiscover: boolean;
  state: AvailabilityFilter;
}

function searchDiscovery({ client, q, type, canDiscover, state }: Readonly<SearchScope>) {
  if (!canDiscover) {
    return Promise.resolve(NO_DISCOVER);
  }
  return client.discovery
    .search(q, { type, state })
    .then((r) => r.results)
    .catch(() => NO_DISCOVER);
}

async function searchEverywhere(scope: Readonly<SearchScope>) {
  const { client, q, type } = scope;
  const [{ results, people }, discover] = await Promise.all([
    client.media.search(q, { limit: 24 }).catch(() => NO_LOCAL),
    searchDiscovery(scope),
  ]);
  return { local: narrowLocal(results, type), people, discover };
}

function useCanDiscover(): boolean {
  const { user } = useAuth();
  return !!user && hasPermission(user, 'requests.create');
}

export function useDiscoverSearch(
  query: string,
  type: DiscoverType,
  state: AvailabilityFilter = 'any',
): DiscoverSearchState {
  const { client } = useWebRuntime();
  const canDiscover = useCanDiscover();
  const q = useDebouncedValue(query.trim(), 250);
  const searching = q.length > 0;

  const { data, isFetching } = useQuery({
    queryKey: ['discoverSearch', q, type, state, canDiscover],
    queryFn: () => searchEverywhere({ client, q, type, state, canDiscover }),
    enabled: searching,
    placeholderData: keepPreviousData,
  });

  return {
    loading: searching && isFetching && data === undefined,
    ...(data ?? NOTHING_FOUND),
    canDiscover,
  };
}

export interface DiscoverFilterState {
  loading: boolean;
  entries: DiscoverEntry[];
  totalResults: number;
  totalPages: number;
}

/** One page of TMDB's filtered browse endpoint. It answers with real totals,
 * so the page can show a count and page forwards without fetching everything.
 * `enabled` is false while a text query is active (TMDB search is its own
 * endpoint) or for an account without `requests.create`. */
export function useDiscoverFilter(search: DiscoverSearch): DiscoverFilterState {
  const { client } = useWebRuntime();
  const canDiscover = useCanDiscover();
  const on =
    search.source === 'tmdb' &&
    search.q.trim().length === 0 &&
    search.type !== 'all' &&
    canDiscover;
  const { data, isFetching } = useQuery({
    queryKey: [
      'discover',
      'filter',
      search.type,
      search.sort,
      search.lang,
      search.year,
      search.decade,
      search.genre,
      search.state,
      search.page,
    ],
    queryFn: () =>
      client.discovery.filter({
        type: search.type === 'tv' ? 'tv' : 'movie',
        sort: search.sort,
        language: search.lang,
        year: search.year,
        decade: search.decade,
        genre: search.genre,
        page: search.page,
        state: search.state,
      }),
    enabled: on,
    placeholderData: keepPreviousData,
  });
  return {
    loading: on && isFetching && data === undefined,
    entries: data?.results ?? [],
    totalResults: data?.totalResults ?? 0,
    totalPages: data?.totalPages ?? 1,
  };
}

export interface LocalDiscoveryState {
  loading: boolean;
  results: SearchHit[];
  totalResults: number;
  totalPages: number;
}

/** One page of the local catalogue discovery grid. The server filters, counts
 * and pages; the client only names the filters. */
export function useLocalDiscovery(search: DiscoverSearch): LocalDiscoveryState {
  const { client } = useWebRuntime();
  const on = search.source === 'library';
  const { data, isFetching } = useQuery({
    queryKey: [
      'discover',
      'local',
      search.q.trim(),
      search.type,
      search.library,
      search.year,
      search.decade,
      search.genre,
      search.rating,
      search.watched,
      search.page,
    ],
    queryFn: () =>
      client.discovery.local({
        q: search.q.trim(),
        type: search.type,
        library: search.library,
        year: search.year,
        decade: search.decade,
        genre: search.genre,
        rating: search.rating,
        watched: search.watched,
        page: search.page,
      }),
    enabled: on,
    placeholderData: keepPreviousData,
  });
  return {
    loading: on && isFetching && data === undefined,
    results: data?.results ?? [],
    totalResults: data?.totalResults ?? 0,
    totalPages: data?.totalPages ?? 1,
  };
}

export interface TrendingState {
  loading: boolean;
  entries: DiscoverEntry[];
}

/** This week's trending movies + shows, for the browse-first empty state.
 * Fetched once when `enabled` (the user can discover). */
export function useTrending(enabled: boolean): TrendingState {
  const webRuntime = useWebRuntime();

  const { data, isFetching } = useQuery({
    queryKey: ['discover', 'trending', 'all'],
    queryFn: () => webRuntime.client.discovery.trending(),
    enabled,
    select: (r) => r.results,
  });
  return { loading: enabled && isFetching && data === undefined, entries: data ?? [] };
}

export interface TrendingPageState {
  loading: boolean;
  entries: DiscoverEntry[];
  totalPages: number;
}

/** One page of trending titles for a single kind (`movie` | `tv`).
 * `keepPreviousData` retains the prior page (and its total) while the next loads. */
export function useTrendingPage(
  type: 'movie' | 'tv',
  page: number,
  enabled: boolean,
): TrendingPageState {
  const webRuntime = useWebRuntime();

  const { data, isFetching } = useQuery({
    ...webRuntime.queries.discoverQueries.trending(type, page),
    enabled,
    placeholderData: keepPreviousData,
  });
  return {
    loading: enabled && isFetching,
    entries: data?.results ?? [],
    totalPages: data?.totalPages ?? 1,
  };
}
