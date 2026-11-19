// Result grids for the discovery page. The library tab draws local catalogue
// hits with the poster the rest of the site uses; the TMDB tab draws discovery
// cards (play / request actions included). The page owns counts, paging and
// empty states, so these stay pure grids.

import type { DiscoverEntry } from '@kromatv/client/discovery';
import type { PersonHit, SearchHit } from '@kromatv/client/media';
import { episodeTag, personSegment, posterColors, sizedImageUrl } from '@kromatv/core';
import { useT } from '@kromatv/ui';
import {
  Avatar,
  Box,
  Focusable,
  Row,
  styles,
  Text,
  useStyles,
} from '@kromatv/ui/kit';
import { DiscoverCard } from '#web/features/requests/discover-card.component';
import { imageUrl } from '#web/shared/lib/api';
import { useAuth } from '#web/shared/lib/auth';
import { EPISODES_ANCHOR } from '#web/shared/lib/show-search';
import { Poster } from '#web/shared/ui/poster.component';
import { RouteLink } from '#web/shared/ui/route-link.component';
import { SkeletonRow } from '#web/shared/ui';
import { TileGrid } from '#web/shared/ui/tile-grid.component';

const sDefinition = styles({
  person: {
    row: true,
    align: 'center',
    gap: 12,
    pl: 6,
    pr: 18,
    py: 6,
    radius: 'pill',
    bg: 'white/6',
    borderWidth: 1,
    borderColor: 'white/10',
    maxW: 280,
  },
});

function LocalHit({ hit, width }: Readonly<{ hit: SearchHit; width: number }>) {
  const { client } = useAuth();
  if (hit.type === 'show') {
    const show = hit.show;
    return (
      <Poster
        title={show.title}
        colors={posterColors(show.id)}
        poster={client.media.artwork.showPosterFor(show)}
        width={width}
        asChild
      >
        <RouteLink to="/shows/$id" params={{ id: show.id }} />
      </Poster>
    );
  }
  const item = hit.item;
  // Episodes route to their show; movies to their own fiche.
  const episode = hit.type === 'episode' && item.showId ? item.showId : null;
  const to = episode ? '/shows/$id' : '/movies/$id';
  return (
    <Poster
      title={item.episodeTitle ?? item.title}
      // An episode wears its show's poster, so it has to say which one it is.
      genre={episode ? [item.showTitle, episodeTag(item)].filter(Boolean).join(' · ') : undefined}
      colors={posterColors(item.id)}
      poster={client.media.artwork.posterFor(item)}
      width={width}
      asChild
    >
      <RouteLink
        to={to}
        params={{ id: episode ?? item.id }}
        search={episode && item.season !== null ? { season: item.season } : {}}
        hash={episode ? EPISODES_ANCHOR : undefined}
      />
    </Poster>
  );
}

const FACE_W = 56;

function PersonChip({ person }: Readonly<{ person: PersonHit }>) {
  const s = useStyles(sDefinition);
  const t = useT();
  return (
    <Focusable label={person.name} style={s.person} asChild>
      <RouteLink to="/people/$person" params={{ person: personSegment(person) }}>
        <Avatar
          name={person.name}
          src={sizedImageUrl(imageUrl(person.profileUrl ?? null), FACE_W)}
          size={FACE_W}
          circle
        />
        <Box minW={0}>
          <Text variant="label" lines={1}>
            {person.name}
          </Text>
          <Text variant="meta" color="textMuted" lines={1}>
            {t('person.titleCount', { count: person.titles })}
          </Text>
        </Box>
      </RouteLink>
    </Focusable>
  );
}

function People({ people }: Readonly<{ people: readonly PersonHit[] }>) {
  const t = useT();
  return (
    <Box mb={24}>
      <Text variant="meta" color="textDim" mb={10}>
        {t('search.people')}
      </Text>
      <Row wrap gap={12}>
        {people.map((person) => (
          <PersonChip key={person.tmdbId ?? person.name} person={person} />
        ))}
      </Row>
    </Box>
  );
}

/** The local tab's grid. Skeletons while the first page loads; an empty grid
 * means the page draws its own empty state (the filters may be too narrow). */
export function LocalResults({
  results,
  loading,
}: Readonly<{ results: readonly SearchHit[]; loading: boolean }>) {
  if (loading) {
    return (
      <Box mt={24}>
        <SkeletonRow count={10} />
      </Box>
    );
  }
  return (
    <TileGrid>
      {(width) =>
        results.map((hit) => {
          const id = hit.type === 'show' ? hit.show.id : hit.item.id;
          return <LocalHit key={`${hit.type}-${id}`} hit={hit} width={width} />;
        })
      }
    </TileGrid>
  );
}

/** The TMDB tab's grid, with the local people hits the query also matched. */
export function TmdbResults({
  entries,
  people = [],
  loading,
}: Readonly<{
  entries: readonly DiscoverEntry[];
  people?: readonly PersonHit[];
  loading: boolean;
}>) {
  if (loading) {
    return (
      <Box mt={24}>
        <SkeletonRow count={10} />
      </Box>
    );
  }
  return (
    <>
      {people.length > 0 ? <People people={people} /> : null}
      <TileGrid>
        {(width) =>
          entries.map((entry) => (
            <DiscoverCard key={`${entry.kind}-${entry.tmdbId}`} entry={entry} width={width} />
          ))
        }
      </TileGrid>
    </>
  );
}
