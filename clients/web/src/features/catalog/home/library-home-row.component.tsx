import type { Library, MediaItem } from '@kromatv/client/media';
import { useT } from '@kromatv/ui';
import { Button, Row, Text } from '@kromatv/ui/kit';
import { useQuery } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';
import { useEffect, useMemo, useRef, useState } from 'react';
import { MoviePoster, ShowPoster } from '#web/features/catalog/cards.component';
import type { MovieCardView, ShowView } from '#web/shared/lib/api';
import { useWebRuntime } from '#web/shared/lib/web-runtime';
import { Poster, PosterRail } from '#web/shared/ui';
import { RouteLink } from '#web/shared/ui/route-link.component';

type Props = Readonly<{ library: Library; viewer: string }>;
type Tile = { type: 'movie'; item: MovieCardView } | { type: 'show'; item: ShowView } | { type: 'photo' | 'other'; item: MediaItem };

// Reserve a row until measured: zero-height sentinels would all intersect at once.
export function LibraryHomeRow(props: Props) {
  const marker = useRef<HTMLDivElement>(null);
  const [near, setNear] = useState(false);
  useEffect(() => {
    if (!marker.current || typeof IntersectionObserver === 'undefined') {
      setNear(true);
      return;
    }
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) {
        setNear(true);
        observer.disconnect();
      }
    }, { rootMargin: '300px' });
    observer.observe(marker.current);
    return () => observer.disconnect();
  }, []);
  return near ? <LibraryContents {...props} /> : <div ref={marker} aria-hidden="true" style={{ minHeight: 280 }} />;
}

function LibraryContents({ library, viewer }: Props) {
  const t = useT();
  const { client, queries: { catalogQueries: q } } = useWebRuntime();
  const { id, kind } = library;
  const films = kind === 'movies' || kind === 'mixed';
  const series = kind === 'shows' || kind === 'mixed';
  const movies = useQuery({ ...q.moviesView(id, viewer), enabled: films });
  const shows = useQuery({ ...q.showsView(id, viewer), enabled: series });
  const photos = useQuery({ ...q.photos(id, undefined, viewer), enabled: kind === 'photos' });
  const other = useQuery({ ...q.other(id, viewer), enabled: kind === 'other' });
  const tiles = useMemo(() => {
    const items: Tile[] = [];
    if (films && movies.isSuccess) for (const item of movies.data) items.push({ type: 'movie', item });
    if (series && shows.isSuccess) for (const item of shows.data) items.push({ type: 'show', item });
    if (kind === 'photos' && photos.isSuccess) for (const item of photos.data) items.push({ type: 'photo', item });
    if (kind === 'other' && other.isSuccess) for (const item of other.data) items.push({ type: 'other', item });
    return items.filter(({ item }) => item.library === id).sort((a, b) =>
      Date.parse(b.item.addedAt) - Date.parse(a.item.addedAt) || a.item.id.localeCompare(b.item.id),
    ).slice(0, 16);
  }, [id, kind, films, series, movies.data, movies.isSuccess, shows.data, shows.isSuccess, photos.data, photos.isSuccess, other.data, other.isSuccess]);
  if (!tiles.length) {
    const active = [films && movies, series && shows, kind === 'photos' && photos, kind === 'other' && other].filter(q => q !== false);
    const failed = active.filter(q => q.isError);
    if (failed.length) return <section aria-label={library.name}><Row wrap between gap={12} mt={40} mb={20}>
      <h2><Link to="/libraries/$id" params={{ id }}><Text variant="h2">{library.name}</Text></Link></h2>
      <Button size="sm" variant="outline" label={t('error.retry')} onPress={() => { for (const result of failed) void result.refetch(); }} />
    </Row><Text variant="meta" color="textMuted">{t('error.serverBody')}</Text></section>;
    return active.some(q => q.isPending) ? <div aria-hidden="true" style={{ minHeight: 280 }} /> : null;
  }
  const destination = { to: '/libraries/$id', params: { id } } as const;
  return <section aria-label={library.name}>
    <Row between gap={12} mt={40} mb={20}>
      <h2><Link {...destination}><Text variant="h2">{library.name}</Text></Link></h2>
      <Link {...destination}><Text variant="meta">{t('discover.seeAll')}</Text></Link>
    </Row>
    <PosterRail data={tiles} renderItem={(tile) => {
      if (tile.type === 'movie') return <MoviePoster item={tile.item} />;
      if (tile.type === 'show') return <ShowPoster show={tile.item} />;
      const photo = tile.type === 'photo';
      return <Poster title={tile.item.title} poster={photo ? client.media.artwork.photoUrl(tile.item.id, 320) : null} asChild>
        {photo ? <RouteLink {...destination} search={{ photo: tile.item.id }} /> : <RouteLink to="/watch/$id" params={{ id: tile.item.id }} />}
      </Poster>;
    }} />
  </section>;
}
