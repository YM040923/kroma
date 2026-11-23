import { type DiscoverEntry, tmdbKindOf } from '@kromatv/client/discovery';
import type { SubjectId } from '@kromatv/client/media';
import type { RequestStatus } from '@kromatv/client/requests';
import { posterColors, sizedImageUrl } from '@kromatv/core';
import type { Translate } from '@kromatv/i18n';
import { useT } from '@kromatv/ui';
import { Box, Img, rhythm, Text } from '@kromatv/ui/kit';
import { type ReactNode, useState } from 'react';
import { imageUrl } from '#web/shared/lib/api';
import { useMyList } from '#web/shared/lib/mylist';
import { savedTitleId } from '#web/shared/lib/saved-title-id';
import { useWatched } from '#web/shared/lib/watched';
import type { PosterAction } from '#web/shared/ui/poster-action-bar.component';
import { PosterTile } from '#web/shared/ui/poster-tile.component';
import { RequestStatusChip } from '#web/shared/ui/request-status-chip.component';
import { RouteLink } from '#web/shared/ui/route-link.component';
import { TileCaption } from '#web/shared/ui/tile-caption.component';

function detailLink(entry: DiscoverEntry, owned: boolean): ReactNode {
  if (owned) {
    return (
      <RouteLink
        to={entry.kind === 'show' ? '/shows/$id' : '/movies/$id'}
        params={{ id: entry.localId ?? '' }}
      />
    );
  }
  return (
    <RouteLink
      to="/discover/$type/$tmdbId"
      params={{ type: tmdbKindOf(entry.kind), tmdbId: String(entry.tmdbId) }}
    />
  );
}

function statusChip(entry: DiscoverEntry, owned: boolean, status: RequestStatus | null): ReactNode {
  if (owned) {
    return <RequestStatusChip status="available" size="card" />;
  }
  if (!status) {
    return null;
  }
  return <RequestStatusChip status={status} size="card" progress={entry.requestProgress} />;
}

interface TileToggles {
  listId: SubjectId | null;
  bookmarked: boolean;
  seen: boolean;
  onToggleList: (id: SubjectId) => void;
  onToggleWatched: (id: SubjectId) => void;
}

function cardActions(t: Translate, toggles: Readonly<TileToggles>): PosterAction[] {
  const { listId, bookmarked, seen } = toggles;
  const actions: PosterAction[] = [];
  if (listId) {
    actions.push(
      {
        key: 'list',
        icon: bookmarked ? 'bookmark-filled' : 'bookmark',
        label: t(bookmarked ? 'content.removeFromList' : 'content.addToList'),
        active: bookmarked,
        onSelect: () => toggles.onToggleList(listId),
      },
      {
        key: 'watched',
        icon: 'eye',
        label: t(seen ? 'content.markUnwatched' : 'content.markWatched'),
        active: seen,
        onSelect: () => toggles.onToggleWatched(listId),
      },
    );
  }
  return actions;
}

export function DiscoverCard({ entry, width }: Readonly<{ entry: DiscoverEntry; width?: number }>) {
  const t = useT();
  const { isWatched, toggleWatched } = useWatched();
  const { inList, toggle: toggleMyList } = useMyList();
  const [imgOk, setImgOk] = useState(true);
  const optimisticStatus = entry.requestStatus;
  const [c1, c2] = posterColors(String(entry.tmdbId));
  const poster = sizedImageUrl(imageUrl(entry.posterUrl), width ?? rhythm.cardWidth);
  const showImg = Boolean(poster) && imgOk;
  const owned = Boolean(entry.inLibrary && entry.localId);
  const listId = savedTitleId(entry.kind, owned ? entry.localId : null, entry.tmdbId);
  const tint = `linear-gradient(158deg, ${c1} 0%, ${c2} 70%)`;

  const seen = listId != null && isWatched(listId);
  const actions = cardActions(t, {
    listId,
    bookmarked: listId != null && inList(listId),
    seen,
    onToggleList: toggleMyList,
    onToggleWatched: toggleWatched,
  });

  return (
    <PosterTile
      label={entry.title}
      asChild
      width={width}
      background={tint}
      watched={seen}
      actions={actions}
      footer={
        <TileCaption
          title={entry.title}
          kind={entry.kind}
          year={entry.year ?? null}
          rating={entry.rating ?? null}
        />
      }
      art={() => (
        <>
          {showImg ? (
            <Img src={poster} alt={entry.title} radius="xl" fill onError={() => setImgOk(false)} />
          ) : (
            <Box fill justify="flex-end" p={12}>
              <Text variant="label" color="white/90" lines={3}>
                {entry.title}
              </Text>
            </Box>
          )}
          <Box absolute left={12} top={12} gap={8}>
            {statusChip(entry, owned, optimisticStatus)}
          </Box>
        </>
      )}
    >
      {detailLink(entry, owned)}
    </PosterTile>
  );
}
