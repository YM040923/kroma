import { tmdbKindOf } from '@kromatv/client/discovery';
import { ModuleSlot } from '@kromatv/module-sdk';
import type { ItemId, SubjectId } from '@kromatv/client/media';
import { formatRuntime } from '@kromatv/core';
import { useT } from '@kromatv/ui';
import type { HostElement } from '@kromatv/ui/kit';
import { ArtworkButton } from '#web/features/catalog/artwork-picker.component';
import {
  audioFlagLabel,
  audioString,
  DetailHero,
  langName,
  qualityBadges,
  subString,
} from '#web/features/catalog/detail.component';
import { ReportDialog } from '#web/features/catalog/report-dialog.component';
import { TrailerButton } from '#web/features/catalog/trailer-button.component';
import { savedTitleId } from '#web/shared/lib/saved-title-id';
import { type TitleView, tmdbMetaLine } from '#web/shared/lib/title-view';

export function TitleHero({
  view,
  owned,
  localId,
  overline,
  isWatched,
  toggleWatched,
  inList,
  toggleList,
  onPlay,
  back,
}: Readonly<{
  view: TitleView;
  owned: boolean;
  localId: string | null | undefined;
  overline: string;
  isWatched: (id: SubjectId) => boolean;
  toggleWatched: (id: SubjectId) => void;
  inList: (id: SubjectId) => boolean;
  toggleList: (id: SubjectId) => void;
  onPlay: (id: ItemId) => void;
  back: HostElement;
}>) {
  const t = useT();
  const playable = owned ? view.playable : null;
  const listId = savedTitleId(view.kind, localId, view.tmdbId);
  const listState: {
    watched?: boolean;
    onToggleWatched?: () => void;
    inList?: boolean;
    onToggleList?: () => void;
  } = {};
  if (listId) {
    listState.watched = isWatched(listId);
    listState.onToggleWatched = () => toggleWatched(listId);
    listState.inList = inList(listId);
    listState.onToggleList = () => toggleList(listId);
  }
  const trackInfo: { audio?: string; subtitles?: string } = playable
    ? { audio: audioString(t, playable), subtitles: subString(t, playable) }
    : {};
  return (
    <DetailHero
      art={{
        id: localId ?? String(view.tmdbId ?? view.title),
        backdrop: view.backdrop,
        poster: view.poster,
      }}
      overline={overline}
      title={view.title}
      rating={view.rating}
      meta={metaLine(t, view)}
      badges={view.video ? qualityBadges(view.video) : []}
      audioFlag={owned ? audioFlagLabel(t, view.playable) : null}
      directors={view.directors}
      tagline={view.tagline}
      overview={view.overview}
      audio={trackInfo.audio}
      subtitles={trackInfo.subtitles}
      playable={playable}
      playLabel={view.playLabel ?? undefined}
      themeUrl={view.themeUrl}
      watched={listState.watched}
      castItemId={playable?.id}
      onToggleWatched={listState.onToggleWatched}
      inList={listState.inList}
      onToggleList={listState.onToggleList}
      primaryAction={undefined}
      trailerAction={
        view.tmdbId == null ? null : (
          <TrailerButton kind={tmdbKindOf(view.kind)} tmdbId={view.tmdbId} title={view.title} />
        )
      }
      adminAction={
        <>
          <ModuleSlot
            name="title.detail.actions"
            kind={view.kind === 'show' ? 'show' : 'movie'}
            tmdbId={view.tmdbId}
            title={view.title}
            year={view.year}
            poster={view.poster}
            owned={owned}
          />
          {localId ? <ArtworkButton itemId={localId} title={view.title} /> : null}
        </>
      }
      back={back}
      onPlay={playable ? () => onPlay(playable.id) : undefined}
      onReport={
        owned && localId
          ? () =>
              void ReportDialog.call({
                subjectKind: view.kind,
                subjectId: localId,
                subjectTitle: view.title,
              })
          : undefined
      }
    />
  );
}

function metaLine(t: ReturnType<typeof useT>, view: TitleView): string {
  if (view.kind === 'show') {
    const episodes = view.seasons.reduce((n, s) => n + (s.episodes.length || s.episodeCount), 0);
    return [
      view.year ? String(view.year) : null,
      t('content.seasonCount', { count: view.seasons.length }),
      t('content.episodeCount', { count: episodes }),
    ]
      .filter(Boolean)
      .join(' · ');
  }
  if (view.playable) {
    return [
      view.year ? String(view.year) : null,
      formatRuntime(view.playable.durationMs),
      view.playable.metadata?.certification,
      langName(t, view.playable.audio?.language),
    ]
      .filter(Boolean)
      .join(' · ');
  }
  return tmdbMetaLine(view.year, view.runtimeMin);
}
