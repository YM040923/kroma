import { useWebRuntime } from '#web/shared/lib/web-runtime';
// The single detail page for a title, owned or not. Fed a normalized `TitleView`

import { tmdbKindOf } from '@kromatv/client/discovery';
import { ItemId, type SubjectId } from '@kromatv/client/media';
import { useCast, useT } from '@kromatv/ui';
import { classes, type HostElement, styles, Text, useStyles } from '@kromatv/ui/kit';
import { useQuery } from '@tanstack/react-query';
import { useNavigate } from '@tanstack/react-router';
import { AiSuggestRail } from '#web/features/catalog/ai-suggest-rail.component';
import { AcquirePanel } from '#web/features/catalog/acquire-panel.component';
import { CastRail, type SimilarItem, SimilarRail } from '#web/features/catalog/detail.component';
import { SeasonSection } from '#web/features/catalog/episode-list.component';
import { TitleHero } from '#web/features/catalog/title-hero.component';
import { SharePanel } from '#web/features/catalog/share-panel.component';
import { TreatmentsPanel } from '#web/features/catalog/treatments-panel.component';
import { useTitleCommands } from '#web/features/catalog/use-title-commands.hook';
import { useTitleRequest } from '#web/features/catalog/use-title-request.hook';
import { useAuth } from '#web/shared/lib/auth';
import { useMyList } from '#web/shared/lib/mylist';

import type { SimilarTarget, TitleView } from '#web/shared/lib/title-view';
import { useWatched } from '#web/shared/lib/watched';
import { page } from '#web/shared/ui';
import { RouteLink } from '#web/shared/ui/route-link.component';

// The page gutter is a fluid CSS custom property, which no style number can
// carry, so anything indented by it stays a plain element.

const sDefinition = styles({
  page: {
    minWidth: 0,
    pb: 80,
    animationKeyframes: 'fade-in',
    animationDuration: '400ms',
    animationTimingFunction: 'ease',
  },
});

type ProgressEntry = { itemId: string; positionMs: number; durationMs?: number | null };

function progressMap(entries: readonly ProgressEntry[]): Record<string, number> {
  const map: Record<string, number> = {};
  for (const e of entries) {
    const dur = e.durationMs ?? 0;
    if (dur > 0 && e.positionMs > 0) {
      map[e.itemId] = Math.min(100, Math.round((e.positionMs / dur) * 100));
    }
  }
  return map;
}

function similarItemsOf(targets: readonly SimilarTarget[]): SimilarItem[] {
  return targets.map((entry) => ({
    id: entry.key,
    title: entry.title,
    genre: entry.genre,
    badge: null,
    poster: entry.poster,
    link: similarLink(entry),
  }));
}

function similarLink(target: SimilarTarget): HostElement | undefined {
  if (target.localId) {
    return (
      <RouteLink
        to={target.kind === 'show' ? '/shows/$id' : '/movies/$id'}
        params={{ id: target.localId }}
      />
    );
  }
  if (target.tmdbId == null) {
    return undefined;
  }
  return (
    <RouteLink
      to="/discover/$type/$tmdbId"
      params={{ type: tmdbKindOf(target.kind), tmdbId: String(target.tmdbId) }}
    />
  );
}

function TitleBody({
  view,
  owned,
  localId,
  error,
  similarItems,
  collectionItems,
  collectionTitle,
  epProgress,
  busy,
  pendingEps,
  selected,
  isWatched,
  toggleWatched,
  onPlay,
  onToggleEpisode,
  onRequestSelected,
  onRequestSeason,
  season,
  onRequestAll,
  onClearSelection,
}: Readonly<{
  view: TitleView;
  owned: boolean;
  localId: string | null | undefined;
  error: string | null;
  similarItems: SimilarItem[];
  collectionItems: SimilarItem[];
  collectionTitle: string;
  epProgress: Record<string, number>;
  busy: boolean;
  pendingEps: Set<string>;
  selected: Set<string>;
  isWatched: (id: SubjectId) => boolean;
  toggleWatched: (id: SubjectId) => void;
  onPlay: (id: ItemId) => void;
  onToggleEpisode: (season: number, episode: number) => void;
  onRequestSelected: () => void;
  onRequestSeason: (season: number) => void;
  season: number | undefined;
  onRequestAll: () => void;
  onClearSelection: () => void;
}>) {
  const pageScoped = useStyles(page);

  const t = useT();
  return (
    <>
      {error ? (
        <div className={classes(pageScoped.gutter)}>
          <Text variant="label" color="dangerHover" mt={8}>
            {error}
          </Text>
        </div>
      ) : null}

      {owned && localId ? (
        <TreatmentsPanel
          kind={view.kind === 'show' ? 'show' : 'item'}
          id={localId}
          title={view.title}
        />
      ) : null}

      {owned && localId && view.kind === 'movie' ? <SharePanel id={localId} /> : null}

      {view.kind === 'movie' ? (
        <CastRail cast={view.cast} />
      ) : (
        <SeasonSection
          key={season}
          seasons={view.seasons}
          openOn={season}
          poster={view.poster || null}
          overview={view.overview}
          year={view.year}
          discoverTmdbId={owned ? null : view.tmdbId}
          fallbackCast={view.cast}
          isWatched={isWatched}
          toggleWatched={toggleWatched}
          progressOf={(id) => epProgress[id] ?? null}
          onPlay={onPlay}
          canRequest={view.canRequest}
          selected={selected}
          onToggleEpisode={onToggleEpisode}
          onRequestSelected={onRequestSelected}
          onRequestSeason={onRequestSeason}
          onRequestAll={onRequestAll}
          onClearSelection={onClearSelection}
          pendingEpisodes={pendingEps}
          requestBusy={busy}
        />
      )}

      {collectionItems.length ? (
        <SimilarRail title={collectionTitle} items={collectionItems} />
      ) : null}

      <SimilarRail title={t('content.similarTitles')} items={similarItems} />

      {owned && localId ? <AiSuggestRail id={ItemId.parse(localId)} /> : null}
    </>
  );
}

export function TitleDetail({
  initial,
  season,
  similar,
  collection,
}: Readonly<{
  initial: TitleView;
  season?: number;
  similar?: readonly SimilarTarget[];
  /** The film's series, when it has one: its title and its other films. */
  collection?: { name: string | null; targets: readonly SimilarTarget[] } | null;
}>) {
  const webRuntime = useWebRuntime();

  const s = useStyles(sDefinition);

  const t = useT();
  const { user, client } = useAuth();
  const navigate = useNavigate();
  const {
    view,
    busy,
    error,
    selected,
    pendingEps,
    panelOpen,
    closePanel,
    takeFromPanel,
    toggleEpisode,
    requestSelected,
    requestSeason,
    requestAllSeasons,
    clearSelection,
    onRequestClick,
  } = useTitleRequest(initial);
  const { isWatched, toggleWatched } = useWatched();
  const { inList, toggle: toggleList } = useMyList();

  const owned = view.localId != null && view.playable != null;
  const localId = view.localId;
  let backTo: '/' | '/shows' | '/search' = '/search';
  if (owned) {
    backTo = view.kind === 'show' ? '/shows' : '/';
  }

  const { data: epProgress = {} } = useQuery({
    ...webRuntime.queries.userQueries.progress(),
    enabled: !!user && !!localId && view.kind === 'show',
    select: (entries) => progressMap(entries),
  });

  const { active: castDevice, playOn } = useCast();
  const play = (id: ItemId) => {
    if (castDevice) {
      void playOn(castDevice.id, id);
      return;
    }
    void navigate({ to: '/watch/$id', params: { id } });
  };
  useTitleCommands(view, play);

  const fallbackOverlineKey = view.kind === 'show' ? 'content.series' : 'content.film';
  const overline = view.genres.length
    ? view.genres.slice(0, 3).join(' · ')
    : t(fallbackOverlineKey);
  const similarItems = similarItemsOf(similar ?? view.similar);
  const collectionItems = similarItemsOf(collection?.targets ?? []);
  const collectionTitle = collection?.name?.trim() || t('content.collection');

  return (
    <main className={classes(s.page)}>
      <TitleHero
        view={view}
        owned={owned}
        localId={localId}
        busy={busy}
        overline={overline}
        isWatched={isWatched}
        toggleWatched={toggleWatched}
        inList={inList}
        toggleList={toggleList}
        onPlay={play}
        onRequest={onRequestClick}
        back={<RouteLink to={backTo} />}
      />

      <TitleBody
        view={view}
        owned={owned}
        localId={localId}
        error={error}
        similarItems={similarItems}
        collectionItems={collectionItems}
        collectionTitle={collectionTitle}
        epProgress={epProgress}
        busy={busy}
        pendingEps={pendingEps}
        selected={selected}
        isWatched={isWatched}
        toggleWatched={toggleWatched}
        onPlay={play}
        onToggleEpisode={toggleEpisode}
        onRequestSelected={requestSelected}
        onRequestSeason={requestSeason}
        season={season}
        onRequestAll={requestAllSeasons}
        onClearSelection={clearSelection}
      />

      <AcquirePanel
        open={panelOpen}
        onClose={closePanel}
        client={client}
        ask={
          view.tmdbId == null
            ? null
            : {
                target: {
                  kind: view.kind,
                  tmdbId: view.tmdbId,
                  title: view.title,
                  year: view.year ?? null,
                },
              }
        }
        onTake={takeFromPanel}
      />
    </main>
  );
}
