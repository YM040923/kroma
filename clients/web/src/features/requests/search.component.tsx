// The discovery page: two server-backed sources. The library tab browses this
// server's own catalogue with server-side filters; the TMDB tab browses TMDB's
// discover endpoint (or searches it with a text query). Filters are URL state,
// counts and totals come from the server, and paging asks for the next slice
// instead of filtering a whole download in the browser.

import { hasPermission } from '@kromatv/client/accounts';
import type { MessageKey } from '@kromatv/i18n';
import { useQuery } from '@tanstack/react-query';
import { useT } from '@kromatv/ui';
import {
  Box,
  Button,
  classes,
  EmptyState,
  Icon,
  InputGroup,
  PageHeader,
  Row,
  SegmentGroup,
  styles,
  Text,
  useStyles,
} from '@kromatv/ui/kit';
import type { ReactNode } from 'react';
import { DiscoverFilterBar } from '#web/features/requests/discover-filter-bar.component';
import {
  DEFAULT_DISCOVER_SEARCH,
  type DiscoverSearch,
  type DiscoverSource,
} from '#web/features/requests/discover-search';
import { DoubanResults } from '#web/features/requests/douban-results.component';
import { LocalResults, TmdbResults } from '#web/features/requests/search-results.component';
import { TrendingBrowse } from '#web/features/requests/trending.component';
import {
  useDiscoverFilter,
  useDiscoverSearch,
  useLocalDiscovery,
  useTrending,
} from '#web/features/requests/use-discover-search.hook';
import { useAuth } from '#web/shared/lib/auth';
import { useWebRuntime } from '#web/shared/lib/web-runtime';

const SOURCES: {
  value: DiscoverSource | 'douban';
  labelKey: MessageKey;
  disabled?: boolean;
}[] = [
  { value: 'library', labelKey: 'discover.source.library' },
  { value: 'tmdb', labelKey: 'discover.source.tmdb' },
  { value: 'douban', labelKey: 'discover.source.douban' },
];

const GUTTER = { paddingLeft: 'var(--gutter-web)', paddingRight: 'var(--gutter-web)' } as const;

const sDefinition = styles({
  searchBox: { width: '100%', maxWidth: 672 },
  page: { minWidth: 0, pb: 80 },
  gutter: GUTTER,
  hero: { ...GUTTER, position: 'relative', pt: 36 },
  washClip: { fill: true, overflow: 'hidden', pointerEvents: 'none' },
  wash: {
    position: 'absolute',
    left: 0,
    right: 0,
    top: -80,
    height: 288,
    backgroundImage:
      'radial-gradient(48% 60% at 28% 20%, color-mix(in srgb, var(--kroma-accent-wash) 10%, transparent), transparent 70%)',
  },
  aboveWash: { position: 'relative' },
  count: { fontVariant: ['tabular-nums'] },
});

function ResultCount({
  count,
  page,
  totalPages,
}: Readonly<{ count: number; page: number; totalPages: number }>) {
  const s = useStyles(sDefinition);
  const t = useT();
  return (
    <Row between gap={12} mt={28} mb={14}>
      <Text variant="meta" color="textDim" style={s.count}>
        {t('discover.resultCount', { count })}
      </Text>
      {totalPages > 1 ? (
        <Text variant="meta" color="textDim" style={s.count}>
          {t('discover.pageOf', { page, total: totalPages })}
        </Text>
      ) : null}
    </Row>
  );
}

function Pager({
  page,
  totalPages,
  onPage,
}: Readonly<{ page: number; totalPages: number; onPage: (page: number) => void }>) {
  const t = useT();
  if (totalPages <= 1) {
    return null;
  }
  return (
    <Row center gap={12} mt={28}>
      <Button
        variant="ghost"
        size="sm"
        icon="chevron-left"
        label={t('discover.prev')}
        disabled={page <= 1}
        onPress={() => onPage(page - 1)}
      />
      <Text variant="meta" color="textDim">
        {t('discover.pageOf', { page, total: totalPages })}
      </Text>
      <Button
        variant="ghost"
        size="sm"
        iconRight="chevron-right"
        label={t('discover.next')}
        disabled={page >= totalPages}
        onPress={() => onPage(page + 1)}
      />
    </Row>
  );
}

function NoMatches() {
  const t = useT();
  return (
    <EmptyState.Root icon="mood-empty">
      <EmptyState.Title>{t('discover.noResults')}</EmptyState.Title>
      <EmptyState.Hint>{t('discover.noResultsHint')}</EmptyState.Hint>
    </EmptyState.Root>
  );
}

export function SearchPage({
  search: incoming,
  onChange,
}: Readonly<{
  search: Partial<DiscoverSearch> & Pick<DiscoverSearch, 'q' | 'type'>;
  onChange: (patch: Partial<DiscoverSearch>) => void;
}>) {
  const search: DiscoverSearch = { ...DEFAULT_DISCOVER_SEARCH, ...incoming };
  const s = useStyles(sDefinition);

  const t = useT();
  const { user } = useAuth();
  const webRuntime = useWebRuntime();
  const canDiscover = !!user && hasPermission(user, 'requests.create');
  const { data: libraries = [] } = useQuery({
    ...webRuntime.queries.catalogQueries.libraries(),
    enabled: !!user && search.source === 'library',
  });
  const state = useDiscoverSearch(search.q, search.type, search.state);
  const trending = useTrending(canDiscover && search.source === 'tmdb');
  const filtered = useDiscoverFilter(search);
  const localState = useLocalDiscovery(search);
  const searching = search.q.trim().length > 0;
  const setPage = (page: number) => onChange({ page });

  // Page body: the source tab decides who answers. On TMDB, a text query uses
  // the search endpoint (which can span both kinds), an empty query with a
  // kind uses discover, and an empty query on "all" keeps the trending browse.
  let body: ReactNode;
  if (search.source === 'douban') {
    body = searching ? (
      <DoubanResults q={search.q} />
    ) : (
      <EmptyState.Root icon="search">
        <EmptyState.Title>{t('discover.doubanHint')}</EmptyState.Title>
      </EmptyState.Root>
    );
  } else if (search.source === 'library') {
    if (localState.loading) {
      body = <LocalResults results={[]} loading />;
    } else if (localState.results.length === 0) {
      body = <NoMatches />;
    } else {
      body = (
        <>
          <ResultCount
            count={localState.totalResults}
            page={search.page}
            totalPages={localState.totalPages}
          />
          <LocalResults results={localState.results} loading={false} />
          <Pager page={search.page} totalPages={localState.totalPages} onPage={setPage} />
        </>
      );
    }
  } else if (searching) {
    if (state.loading) {
      body = <TmdbResults entries={[]} loading />;
    } else if (state.discover.length === 0 && state.people.length === 0) {
      body = <NoMatches />;
    } else {
      body = (
        <>
          {state.canDiscover && state.discover.length > 0 ? (
            <ResultCount
              count={state.discover.length}
              page={1}
              totalPages={1}
            />
          ) : null}
          <TmdbResults
            entries={state.discover}
            people={state.people}
            loading={false}
          />
        </>
      );
    }
  } else if (!canDiscover) {
    body = (
      <EmptyState.Root icon="mood-empty">
        <EmptyState.Title>{t('discover.empty')}</EmptyState.Title>
      </EmptyState.Root>
    );
  } else if (search.type === 'all') {
    body = <TrendingBrowse entries={trending.entries} loading={trending.loading} type="all" />;
  } else if (filtered.loading) {
    body = <TmdbResults entries={[]} loading />;
  } else if (filtered.entries.length === 0) {
    body = <NoMatches />;
  } else {
    body = (
      <>
        <ResultCount
          count={filtered.totalResults}
          page={search.page}
          totalPages={filtered.totalPages}
        />
        <TmdbResults entries={filtered.entries} loading={false} />
        <Pager page={search.page} totalPages={filtered.totalPages} onPage={setPage} />
      </>
    );
  }

  return (
    <main className={classes(s.page)}>
      <div className={classes(s.hero)}>
        <div className={classes(s.washClip)}>
          <div className={classes(s.wash)} />
        </div>
        <div className={classes(s.aboveWash)}>
          <PageHeader.Root>
            <PageHeader.Title>{t('discover.title')}</PageHeader.Title>
            <PageHeader.Subtitle>
              {canDiscover ? t('discover.subtitle') : t('discover.subtitleLocal')}
            </PageHeader.Subtitle>
          </PageHeader.Root>

          <Box row wrap align="center" gap={12} mt={24}>
            <InputGroup.Root size="md" label={t('discover.title')} style={s.searchBox}>
              <InputGroup.Addon>
                <Icon name="search" size={20} color="textDim" />
              </InputGroup.Addon>
              <InputGroup.Input
                type="search"
                value={search.q}
                onValueChange={(q) => onChange({ q, page: 1 })}
                placeholder={t('discover.placeholder')}
              />
              {search.q ? (
                <InputGroup.Addon align="inline-end">
                  <InputGroup.IconButton
                    icon="x"
                    label={t('common.clear')}
                    onPress={() => onChange({ q: '', page: 1 })}
                  />
                </InputGroup.Addon>
              ) : null}
            </InputGroup.Root>

            <SegmentGroup.Root
              size="md"
              label={t('discover.source')}
              value={search.source}
              onValueChange={(v) =>
                onChange({ source: v as DiscoverSource, page: 1 })
              }
            >
              {SOURCES.map((source) => (
                <SegmentGroup.Item
                  key={source.value}
                  value={source.value}
                  disabled={source.disabled}
                >
                  <SegmentGroup.Label>{t(source.labelKey)}</SegmentGroup.Label>
                </SegmentGroup.Item>
              ))}
            </SegmentGroup.Root>
          </Box>

          <DiscoverFilterBar
            search={search}
            canDiscover={canDiscover}
            libraries={libraries}
            onChange={onChange}
          />

          {search.source === 'tmdb' ? (
            <Text variant="meta" color="textDim" mt={10}>
              {t('discover.source.doubanUnavailable')}
            </Text>
          ) : null}
        </div>
      </div>

      <div className={classes(s.gutter)}>{body}</div>
    </main>
  );
}
