// The discovery page's filter chips: a source tab chooses which server answers,
// and each tab shows only the filters its endpoint understands. Every control
// writes the URL (the page owns navigation), so a filtered view is shareable
// and survives a refresh.

import type { DiscoverType } from '@kromatv/client/discovery';
import { GENRES, type GenreSlug, genreLabel } from '@kromatv/core';
import type { MessageKey } from '@kromatv/i18n';
import { useT } from '@kromatv/ui';
import { Box, Chip, Row, Select, Text } from '@kromatv/ui/kit';
import {
  activeFilterCount,
  clearedDiscoverSearch,
  type DiscoverSearch,
  type DiscoverSort,
  type DiscoverSource,
} from '#web/features/requests/discover-search';

const TYPES: { value: DiscoverType; labelKey: MessageKey }[] = [
  { value: 'all', labelKey: 'discover.all' },
  { value: 'movie', labelKey: 'discover.movies' },
  { value: 'tv', labelKey: 'discover.shows' },
];

const SORTS: { value: DiscoverSort; labelKey: MessageKey }[] = [
  { value: 'popularity', labelKey: 'discover.sort.popularity' },
  { value: 'release', labelKey: 'discover.sort.release' },
  { value: 'rating', labelKey: 'discover.sort.rating' },
];

const LANGUAGES = [
  'fr',
  'en',
  'ja',
  'zh',
  'ko',
  'es',
  'de',
  'it',
  'hi',
  'ru',
  'pt',
  'ar',
  'tr',
] as const;

const RATINGS = [9, 8, 7] as const;

// TMDB's genre ids are not interchangeable between the two endpoints: a movie
// list ignores Action & Adventure, a TV list ignores Fantasy.
const MOVIE_GENRES = new Set([
  28, 12, 16, 35, 80, 99, 18, 10751, 14, 36, 27, 10402, 9648, 10749, 878, 10770, 53, 10752, 37,
]);
const TV_GENRES = new Set([
  10759, 16, 35, 80, 99, 18, 10751, 10762, 9648, 10763, 10764, 10765, 10766, 10767, 10768, 37,
]);

const ALL = '*';
const INLINE_GENRES = 10;
const CURRENT_YEAR = new Date().getFullYear();
const YEARS: number[] = Array.from({ length: CURRENT_YEAR - 1949 }, (_, i) => CURRENT_YEAR - i);
const DECADES: number[] = Array.from(
  { length: Math.floor((CURRENT_YEAR - 1950) / 10) + 1 },
  (_, i) => CURRENT_YEAR - (CURRENT_YEAR % 10) - i * 10,
);

const genreRows = (type: DiscoverType, source: DiscoverSource) => {
  if (source === 'library') {
    return GENRES;
  }
  const ids = type === 'tv' ? TV_GENRES : MOVIE_GENRES;
  return GENRES.filter((g) => ids.has(g.tmdb));
};

function genreChipLabel(t: ReturnType<typeof useT>, slug: string): string {
  return genreLabel(t, slug);
}

export interface DiscoverFilterBarProps {
  search: DiscoverSearch;
  canDiscover: boolean;
  libraries: readonly { id: string; name: string }[];
  /** A patch always resets the page: the result set behind page 5 is gone. */
  onChange: (patch: Partial<DiscoverSearch>) => void;
}

export function DiscoverFilterBar({
  search,
  canDiscover,
  libraries,
  onChange,
}: Readonly<DiscoverFilterBarProps>) {
  const t = useT();
  const tmdb = search.source === 'tmdb';
  const searching = search.q.trim().length > 0;
  const discoverReady = tmdb && !searching && search.type !== 'all';
  const filters = activeFilterCount(search);
  const kinds = TYPES;
  const genres = genreRows(search.type, search.source);
  const genreSlug = search.genre !== undefined ? String(search.genre) : undefined;
  const inlineGenres = genres.slice(0, INLINE_GENRES);
  if (genreSlug && !inlineGenres.some((g) => String(g.tmdb) === genreSlug)) {
    const extra = genres.find((g) => String(g.tmdb) === genreSlug);
    if (extra) {
      inlineGenres.push(extra);
    }
  }

  return (
    <Box mt={20}>
      <Row wrap gap={8} mb={10} aria-label={t('discover.filters')}>
        {kinds.map((tp) => (
          <Chip
            key={tp.value}
            active={search.type === tp.value}
            variant="subtle"
            label={t(tp.labelKey)}
            onPress={() => onChange({ type: tp.value })}
          />
        ))}
        {discoverReady
          ? SORTS.map((sort) => (
              <Chip
                key={sort.value}
                active={search.sort === sort.value}
                variant="subtle"
                label={t(sort.labelKey)}
                onPress={() => onChange({ sort: sort.value })}
              />
            ))
          : null}
        {tmdb ? (
          <>
            <Chip
              active={search.state === 'any'}
              variant="subtle"
              label={t('discover.all')}
              onPress={() => onChange({ state: 'any' })}
            />
            <Chip
              active={search.state === 'in'}
              variant="subtle"
              label={t('discover.state.in')}
              onPress={() => onChange({ state: 'in' })}
            />
            <Chip
              active={search.state === 'out'}
              variant="subtle"
              label={t('discover.state.out')}
              onPress={() => onChange({ state: 'out' })}
            />
          </>
        ) : (
          <>
            <Chip
              active={search.watched === 'any'}
              variant="subtle"
              label={t('discover.all')}
              onPress={() => onChange({ watched: 'any' })}
            />
            <Chip
              active={search.watched === 'watched'}
              variant="subtle"
              label={t('discover.watched.watched')}
              onPress={() => onChange({ watched: 'watched' })}
            />
            <Chip
              active={search.watched === 'unwatched'}
              variant="subtle"
              label={t('discover.watched.unwatched')}
              onPress={() => onChange({ watched: 'unwatched' })}
            />
            {RATINGS.map((rating) => (
              <Chip
                key={rating}
                active={search.rating === rating}
                variant="subtle"
                label={t('discover.ratingAtLeast', { rating })}
                onPress={() => onChange({ rating: search.rating === rating ? undefined : rating })}
              />
            ))}
          </>
        )}
        {filters > 0 ? (
          <Chip
            variant="subtle"
            icon="x"
            label={t('discover.clearFilters')}
            onPress={() => onChange(clearedDiscoverSearch(search))}
          />
        ) : null}
      </Row>

      {!tmdb || discoverReady ? (
        <Row wrap gap={8} mb={10} aria-label={t('discover.filter.genre')}>
          <Chip
            active={search.genre === undefined}
            variant="subtle"
            label={t('browse.allGenres')}
            onPress={() => onChange({ genre: undefined })}
          />
          {inlineGenres.map((g) => (
            <Chip
              key={g.slug}
              active={search.genre === g.tmdb}
              variant="subtle"
              label={genreChipLabel(t, g.slug as GenreSlug)}
              onPress={() =>
                onChange({ genre: search.genre === g.tmdb ? undefined : g.tmdb })
              }
            />
          ))}
          <Select.Root
            label={t('discover.filter.genre')}
            placeholder={t('discover.filter.genre')}
            value={genreSlug ?? ALL}
            onValueChange={(v) => onChange({ genre: v === ALL ? undefined : Number(v) })}
          >
            <Select.Trigger size="sm" />
            <Select.Item value={ALL} label={t('browse.allGenres')} />
            {genres.map((g) => (
              <Select.Item
                key={g.slug}
                value={String(g.tmdb)}
                label={genreChipLabel(t, g.slug)}
                />
            ))}
          </Select.Root>
        </Row>
      ) : null}

      {tmdb ? (
        <Row wrap gap={8}>
          {discoverReady ? (
            <>
              <Select.Root
                label={t('discover.filter.language')}
                placeholder={t('discover.filter.language')}
                value={search.lang ?? ALL}
                onValueChange={(v) => onChange({ lang: v === ALL ? undefined : v })}
              >
                <Select.Trigger size="sm" />
                <Select.Item value={ALL} label={t('discover.filter.allLanguages')} />
                {LANGUAGES.map((code) => (
                  <Select.Item
                    key={code}
                    value={code}
                    label={t(`lang.${code}` as MessageKey)}
                  />
                ))}
              </Select.Root>
              <Select.Root
                label={t('discover.filter.year')}
                placeholder={t('discover.filter.year')}
                value={search.year !== undefined ? String(search.year) : ALL}
                onValueChange={(v) =>
                  onChange({ year: v === ALL ? undefined : Number(v), decade: undefined })
                }
              >
                <Select.Trigger size="sm" />
                <Select.Item value={ALL} label={t('discover.filter.allYears')} />
                {YEARS.map((year) => (
                  <Select.Item key={year} value={String(year)} label={String(year)} />
                ))}
              </Select.Root>
              <Select.Root
                label={t('discover.filter.decade')}
                placeholder={t('discover.filter.decade')}
                value={search.decade !== undefined ? String(search.decade) : ALL}
                onValueChange={(v) =>
                  onChange({ decade: v === ALL ? undefined : Number(v), year: undefined })
                }
              >
                <Select.Trigger size="sm" />
                <Select.Item value={ALL} label={t('discover.filter.allDecades')} />
                {DECADES.map((start) => (
                  <Select.Item
                    key={start}
                    value={String(start)}
                    label={t('discover.decadeLabel', { decade: start })}
                  />
                ))}
              </Select.Root>
            </>
          ) : (
            <Text variant="meta" color="textDim">
              {searching ? t('discover.filterSearchHint') : t('discover.filterPickType')}
            </Text>
          )}
        </Row>
      ) : (
        <Row wrap gap={8}>
          <Select.Root
            label={t('discover.filter.year')}
            placeholder={t('discover.filter.year')}
            value={search.year !== undefined ? String(search.year) : ALL}
            onValueChange={(v) =>
              onChange({ year: v === ALL ? undefined : Number(v), decade: undefined })
            }
          >
            <Select.Trigger size="sm" />
            <Select.Item value={ALL} label={t('discover.filter.allYears')} />
            {YEARS.map((year) => (
              <Select.Item key={year} value={String(year)} label={String(year)} />
            ))}
          </Select.Root>
          <Select.Root
            label={t('discover.filter.decade')}
            placeholder={t('discover.filter.decade')}
            value={search.decade !== undefined ? String(search.decade) : ALL}
            onValueChange={(v) =>
              onChange({ decade: v === ALL ? undefined : Number(v), year: undefined })
            }
          >
            <Select.Trigger size="sm" />
            <Select.Item value={ALL} label={t('discover.filter.allDecades')} />
            {DECADES.map((start) => (
              <Select.Item
                key={start}
                value={String(start)}
                label={t('discover.decadeLabel', { decade: start })}
              />
            ))}
          </Select.Root>
        </Row>
      )}

      {!tmdb && libraries.length > 0 ? (
        <Row wrap gap={8} mt={10} aria-label={t('discover.filter.library')}>
          <Chip
            active={search.library === undefined}
            variant="subtle"
            icon="library"
            label={t('browse.allLibraries')}
            onPress={() => onChange({ library: undefined })}
          />
          {libraries.map((lib) => (
            <Chip
              key={lib.id}
              active={search.library === lib.id}
              variant="subtle"
              label={lib.name}
              onPress={() =>
                onChange({ library: search.library === lib.id ? undefined : lib.id })
              }
            />
          ))}
        </Row>
      ) : null}

      {!canDiscover && tmdb ? (
        <Text variant="meta" color="textDim" mt={10}>
          {t('discover.tmdbUnavailable')}
        </Text>
      ) : null}
    </Box>
  );
}
