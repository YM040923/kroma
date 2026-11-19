import { useWebRuntime } from '#web/shared/lib/web-runtime';
// The unified season section. It opens on a season the library actually holds
// (the one being watched, else the first owned one); TMDB-only seasons stay out
// of the switcher until the viewer asks to see them, and then wear an unstored
// outline. A selected gap renders the season card plus its missing episodes, so
// the view is never blank; a partly owned season lists its owned episodes and
// folds the gaps behind one entry. The sticky request bar below turns ticks
// into one request, and the missing-request actions (whole season / every
// missing season) state their scope in a confirmation before they submit.

import type { CastMember, ItemId, MediaItem, SubjectId } from '@kromatv/client/media';
import { useT } from '@kromatv/ui';
import { Badge, Box, Button, Chip, classes, ringRoomBlock, styles, Text, useStyles } from '@kromatv/ui/kit';
import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { CastRail } from '#web/features/catalog/detail.component';
import { EpisodeRow, MissingEpisodeRow } from '#web/features/catalog/episode-row.component';
import { epKey } from '#web/features/catalog/episode-selection';
import { RequestBar, SeasonAsk } from '#web/features/catalog/request-bar.component';
import { SeasonRequestCard } from '#web/features/catalog/season-card.component';
import { EPISODES_ANCHOR } from '#web/shared/lib/show-search';

import type { SeasonGap, TitleSeason } from '#web/shared/lib/title-view';

// The page gutter is a fluid CSS custom property and `overflow-x` names one
// axis, so both stay plain elements around kit content.
const GUTTER = { paddingLeft: 'var(--gutter-web)', paddingRight: 'var(--gutter-web)' } as const;

const sDefinition = styles({
  gutter: GUTTER,
  chipScroller: {
    ...GUTTER,
    ...ringRoomBlock(),
    display: 'flex',
    gap: 8,
    overflowX: 'auto',
    scrollbarWidth: 'none',
  },
  section: { mt: 40 },
});

/** The owned episodes of a season, by episode number. */
export function ownedEpisodeMap(season: TitleSeason): Map<number, MediaItem> {
  const owned = new Map<number, MediaItem>();
  for (const episode of season.episodes) {
    if (episode.episode != null) {
      owned.set(episode.episode, episode);
    }
  }
  return owned;
}

/** A season counts as local once the library holds at least one of its
 * episodes; TMDB-only rows stay out of the default switcher. */
export function hasLocalEpisodes(season: TitleSeason): boolean {
  return season.episodes.length > 0;
}

/** The most recently watched or in-progress season, if any. There are no
 * timestamps on hand, so the highest season with activity wins: a viewer who
 * resumed the newest season is the common case. */
export function resumeSeasonNumber(
  seasons: readonly TitleSeason[],
  isWatched: (id: SubjectId) => boolean,
  progressOf: (id: ItemId) => number | null,
): number | null {
  let best: { season: number; episode: number } | null = null;
  for (const season of seasons) {
    if (!hasLocalEpisodes(season)) {
      continue;
    }
    for (const episode of season.episodes) {
      if (episode.episode == null) {
        continue;
      }
      const inProgress = (progressOf(episode.id) ?? 0) > 0;
      if (!inProgress && !isWatched(episode.id)) {
        continue;
      }
      if (
        best == null ||
        season.number > best.season ||
        (season.number === best.season && episode.episode > best.episode)
      ) {
        best = { season: season.number, episode: episode.episode };
      }
    }
  }
  return best?.season ?? null;
}

/** Where the section opens: an explicit (still valid) season wins, then the
 * watched one, then the first owned season. A TMDB-only season is selected on
 * its own only when the show has no local season at all, or when a deep link
 * names it explicitly. */
export function defaultSeasonNumber(
  seasons: readonly TitleSeason[],
  openOn: number | undefined,
  isWatched: (id: SubjectId) => boolean,
  progressOf: (id: ItemId) => number | null,
): number {
  const local = seasons.filter(hasLocalEpisodes);
  const routed =
    openOn != null && seasons.some((season) => season.number === openOn) ? openOn : undefined;
  if (routed != null && (local.length === 0 || local.some((season) => season.number === routed))) {
    return routed;
  }
  return (
    resumeSeasonNumber(local, isWatched, progressOf) ??
    local[0]?.number ??
    routed ??
    seasons[0]?.number ??
    1
  );
}

/** The gaps to list for a season, newest overlay first (the TMDB season query
 * carries richer titles and stills than the stored guide). */
export function mergeSeasonGaps(
  season: TitleSeason,
  listing: readonly SeasonGap[] | null,
): SeasonGap[] {
  const byEpisode = new Map<number, SeasonGap>();
  for (const gap of season.missing) {
    byEpisode.set(gap.episode, gap);
  }
  for (const gap of listing ?? []) {
    byEpisode.set(gap.episode, gap);
  }
  return [...byEpisode.values()].sort((a, b) => a.episode - b.episode);
}

/** The episode numbers a season is missing. With permission to request, a
 * known episode count enumerates the whole run so the list never depends on
 * TMDB having answered the season query; without it, only the stored guide's
 * gaps are shown. A fully available season has none. */
export function missingEpisodeNumbers(season: TitleSeason, canRequest: boolean): number[] {
  if (season.available) {
    return [];
  }
  const owned = ownedEpisodeMap(season);
  const numbers = new Set<number>();
  for (const gap of season.missing) {
    if (gap.episode >= 1) {
      numbers.add(gap.episode);
    }
  }
  if (canRequest && season.episodeCount > 0) {
    for (let episode = 1; episode <= season.episodeCount; episode += 1) {
      numbers.add(episode);
    }
  }
  return [...numbers].filter((episode) => !owned.has(episode)).sort((a, b) => a - b);
}

/** Which seasons the switcher draws. Missing seasons are opt-in; a show with
 * nothing local opens straight onto the full (missing) list. */
export function visibleSeasons(seasons: readonly TitleSeason[], showAll: boolean): TitleSeason[] {
  const local = seasons.filter(hasLocalEpisodes);
  return showAll || local.length === 0 ? [...seasons] : local;
}

function SeasonSwitcher({
  seasons,
  localNumbers,
  current,
  showAll,
  missingCount,
  onPick,
  onToggleAll,
}: Readonly<{
  seasons: TitleSeason[];
  localNumbers: ReadonlySet<number>;
  current: number;
  showAll: boolean;
  missingCount: number;
  onPick: (n: number) => void;
  onToggleAll: () => void;
}>) {
  const s = useStyles(sDefinition);

  const t = useT();
  return (
    <div className={classes(s.chipScroller)}>
      {seasons.map((one) => {
        const local = localNumbers.has(one.number);
        const label = local
          ? t('content.season', { number: one.number })
          : `${t('content.season', { number: one.number })} · ${t('content.seasonNotInLibrary')}`;
        return (
          <Chip
            key={one.number}
            active={one.number === current}
            variant={local ? 'solid' : 'outline'}
            label={label}
            onPress={() => onPick(one.number)}
          />
        );
      })}
      {localNumbers.size > 0 && missingCount > 0 ? (
        <Chip
          variant="outline"
          active={showAll}
          icon={showAll ? 'eye-off' : 'eye'}
          label={showAll ? t('content.seasonsShowOwned') : t('content.seasonsShowAll')}
          count={showAll ? undefined : missingCount}
          onPress={onToggleAll}
        />
      ) : null}
    </div>
  );
}

export function SeasonSection({
  seasons,
  openOn,
  poster,
  overview,
  year,
  discoverTmdbId,
  fallbackCast,
  isWatched,
  toggleWatched,
  progressOf,
  onPlay,
  canRequest,
  selected,
  onToggleEpisode,
  onRequestSelected,
  onRequestSeason,
  onRequestAll,
  onClearSelection,
  pendingEpisodes,
  requestBusy,
}: Readonly<{
  seasons: TitleSeason[];
  openOn?: number;
  poster: string | null;
  overview: string | null;
  year: number | null;
  discoverTmdbId: number | null;
  fallbackCast: CastMember[];
  isWatched: (id: SubjectId) => boolean;
  toggleWatched: (id: SubjectId) => void;
  progressOf: (id: ItemId) => number | null;
  onPlay: (id: ItemId) => void;
  canRequest: boolean;
  selected: Set<string>;
  onToggleEpisode: (season: number, episode: number) => void;
  onRequestSelected: () => void;
  onRequestSeason: (season: number) => void;
  onRequestAll: () => void;
  onClearSelection: () => void;
  pendingEpisodes: Set<string>;
  requestBusy: boolean;
}>) {
  const webRuntime = useWebRuntime();

  const s = useStyles(sDefinition);

  const t = useT();
  const localNumbers = new Set(seasons.filter(hasLocalEpisodes).map((one) => one.number));
  const missingSeasons = seasons.filter((one) => !hasLocalEpisodes(one));
  const [season, setSeason] = useState(() =>
    defaultSeasonNumber(seasons, openOn, isWatched, progressOf),
  );
  // A deep link to a missing season is explicit, so open the full list on it
  // rather than silently moving the viewer to another season.
  const [showAll, setShowAll] = useState(
    () =>
      localNumbers.size > 0 &&
      openOn != null &&
      localNumbers.has(openOn) === false &&
      seasons.some((one) => one.number === openOn),
  );
  // Only a partly owned season folds its gaps; a fully missing one shows them.
  const [missingOpen, setMissingOpen] = useState(false);
  const { data: listing } = useQuery({
    ...webRuntime.queries.discoverQueries.season(discoverTmdbId ?? 0, season),
    enabled: discoverTmdbId != null,
  });
  const picked = seasons.find((one) => one.number === season) ?? seasons[0];
  const current = picked
    ? { ...picked, missing: mergeSeasonGaps(picked, listing?.episodes ?? null) }
    : undefined;
  // The ticks leave the screen with their season, and the request bar counts
  // the whole set: carrying them over would submit episodes nothing on screen
  // shows, and nothing offers a way to see or clear.
  const pickSeason = (n: number) => {
    if (n === season) {
      return;
    }
    onClearSelection();
    setMissingOpen(false);
    setSeason(n);
  };
  const toggleAll = () => {
    if (showAll) {
      const local = seasons.filter(hasLocalEpisodes);
      if (!local.some((one) => one.number === season)) {
        const next = resumeSeasonNumber(local, isWatched, progressOf) ?? local[0]?.number;
        if (next != null) {
          onClearSelection();
          setSeason(next);
        }
        setMissingOpen(false);
      }
    }
    setShowAll((open) => !open);
  };
  if (!current) {
    return null;
  }

  const visible = visibleSeasons(seasons, showAll);
  const owned = ownedEpisodeMap(current);
  const ownedNumbers = [...owned.keys()].sort((a, b) => a - b);
  const missingNumbers = missingEpisodeNumbers(current, canRequest);
  const local = hasLocalEpisodes(current);
  const partial = local && missingNumbers.length > 0;
  const showMissing = !local || missingOpen;
  const listedBy = new Map(current.missing.map((gap) => [gap.episode, gap]));

  const requestableAll = canRequest
    ? seasons
        .filter((one) => !one.available && !one.requested)
        .map((one) => ({ number: one.number, count: missingEpisodeNumbers(one, true).length }))
        .filter((one) => one.count > 0)
    : [];
  const seasonRequestable =
    canRequest && !current.available && !current.requested && missingNumbers.length > 0;

  const missingRows = missingNumbers.map((n) => (
    <MissingEpisodeRow
      key={`m-${n}`}
      season={current.number}
      episode={n}
      listed={listedBy.get(n) ?? null}
      pending={current.requested || pendingEpisodes.has(epKey(current.number, n))}
      selected={selected.has(epKey(current.number, n))}
      selectable={canRequest}
      poster={poster}
      onToggle={() => onToggleEpisode(current.number, n)}
    />
  ));

  return (
    <section id={EPISODES_ANCHOR} className={classes(s.section)}>
      <div className={classes(s.gutter)}>
        <Box row align="center" between gap={12} mb={16}>
          <h2>
            <Text variant="h2">{t('content.episodes')}</Text>
          </h2>
          <Text variant="meta" color="textDim">
            {missingNumbers.length > 0
              ? t('discover.episodesPartial', {
                  have: String(ownedNumbers.length),
                  total: String(
                    Math.max(current.episodeCount, ownedNumbers.length + missingNumbers.length),
                  ),
                })
              : t('content.episodeCount', { count: ownedNumbers.length })}
          </Text>
        </Box>
      </div>

      {seasons.length > 1 ? (
        <Box mb={8}>
          <SeasonSwitcher
            seasons={visible}
            localNumbers={localNumbers}
            current={current.number}
            showAll={showAll}
            missingCount={missingSeasons.length}
            onPick={pickSeason}
            onToggleAll={toggleAll}
          />
        </Box>
      ) : null}

      <CastRail cast={current.cast.length ? current.cast : fallbackCast} />

      {local ? null : (
        <div className={classes(s.gutter)}>
          <Box mt={16}>
            <SeasonRequestCard season={current} poster={poster} overview={overview} year={year} />
          </Box>
        </div>
      )}

      {ownedNumbers.length > 0 ? (
        <div className={classes(s.gutter)}>
          <Box gap={14} mt={16}>
            {ownedNumbers.map((n) => {
              const episode = owned.get(n);
              if (!episode) {
                return null;
              }
              return (
                <EpisodeRow
                  key={episode.id}
                  episode={episode}
                  watched={isWatched(episode.id)}
                  progress={progressOf(episode.id)}
                  onPlay={() => onPlay(episode.id)}
                  onToggleWatched={() => toggleWatched(episode.id)}
                />
              );
            })}
          </Box>
        </div>
      ) : null}

      {partial ? (
        <div className={classes(s.gutter)}>
          <Box mt={16}>
            <Button
              variant="ghost"
              size="sm"
              icon={missingOpen ? 'chevron-up' : 'chevron-down'}
              label={
                missingOpen
                  ? t('content.hideMissingEpisodes', { count: missingNumbers.length })
                  : t('content.showMissingEpisodes', { count: missingNumbers.length })
              }
              onPress={() => setMissingOpen((open) => !open)}
            />
          </Box>
        </div>
      ) : null}

      {showMissing && missingNumbers.length > 0 ? (
        <div className={classes(s.gutter)}>
          <Box mt={16} gap={12}>
            <Box row align="center" between gap={12}>
              <Text variant="label" color="textMuted">
                {t('content.missingEpisodes')}
              </Text>
              <Box row align="center" gap={8}>
                <Badge tone="neutral">{t('content.seasonNotInLibrary')}</Badge>
                <Text variant="meta" color="textDim">
                  {t('content.episodeCount', { count: missingNumbers.length })}
                </Text>
              </Box>
            </Box>
            <Text variant="meta" color="textDim">
              {t('content.missingEpisodesHint')}
            </Text>
            <Box gap={10}>{missingRows}</Box>
          </Box>
        </div>
      ) : null}

      {canRequest && (seasonRequestable || requestableAll.length > 0) ? (
        <div className={classes(s.gutter)}>
          <Box mt={20} gap={8}>
            <SeasonAsk
              season={current.number}
              seasonMissing={missingNumbers.length}
              seasonRequestable={seasonRequestable}
              allMissing={requestableAll}
              busy={requestBusy}
              onRequestSeason={() => onRequestSeason(current.number)}
              onRequestAll={onRequestAll}
            />
            <Text variant="meta" color="textDim">
              {t('requests.requestSkipNote')}
            </Text>
          </Box>
        </div>
      ) : null}

      {canRequest ? (
        <RequestBar
          count={selected.size}
          busy={requestBusy}
          onRequestSelected={onRequestSelected}
          onClear={onClearSelection}
        />
      ) : null}
    </section>
  );
}
