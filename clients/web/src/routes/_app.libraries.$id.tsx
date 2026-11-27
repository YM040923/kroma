import type { Library } from '@kromatv/client/media';
import { compareTitles } from '@kromatv/core';
import { useT } from '@kromatv/ui';
import { Box, Button, EmptyState, PageHeader } from '@kromatv/ui/kit';
import { useQuery } from '@tanstack/react-query';
import { createFileRoute } from '@tanstack/react-router';
import { useMemo } from 'react';
import { CatalogGrid, type CatalogEntry } from '#web/features/catalog/cards.component';
import { useAuth } from '#web/shared/lib/auth';
import { useWebRuntime } from '#web/shared/lib/web-runtime';
import { PageFrame, SkeletonRow } from '#web/shared/ui';
import { OtherRow } from './_app.other';
import { PhotoGrid } from './_app.photos';

export const Route = createFileRoute('/_app/libraries/$id')({
  validateSearch: (raw: Record<string, unknown>): { photo?: string } =>
    typeof raw.photo === 'string' && raw.photo ? { photo: raw.photo } : {},
  component: LibraryPage,
});

type Scope = Readonly<{ library: Library; viewer: string }>;

function LibraryPage() {
  const { id } = Route.useParams();
  const { user } = useAuth();
  return user?.id ? <VisibleLibrary key={JSON.stringify([user.id, id])} id={id} viewer={user.id} /> : <SkeletonRow count={8} />;
}

function VisibleLibrary({ id, viewer }: Readonly<{ id: string; viewer: string }>) {
  const { queries } = useWebRuntime();
  // Revalidate even a warm grant list before mounting a scoped content query.
  const grants = useQuery({ ...queries.catalogQueries.libraries(viewer), refetchOnMount: 'always' });
  if (grants.isError) return <PageFrame><Notice state="error" retry={() => void grants.refetch()} /></PageFrame>;
  if (grants.isPending || grants.fetchStatus !== 'idle') return <PageFrame><SkeletonRow count={8} /></PageFrame>;
  const library = grants.data.find((entry) => entry.id === id);
  if (!library) return <PageFrame><Notice state="missing" /></PageFrame>;
  return <PageFrame key={JSON.stringify([viewer, id])}>
    <PageHeader.Root><PageHeader.Title>{library.name}</PageHeader.Title></PageHeader.Root>
    {library.kind === 'photos' ? <Photos library={library} viewer={viewer} />
      : library.kind === 'other' ? <Other library={library} viewer={viewer} />
      : <Titles library={library} viewer={viewer} />}
  </PageFrame>;
}

function Notice({ state, retry }: Readonly<{ state: 'missing' | 'error' | 'empty'; retry?: () => void }>) {
  const t = useT();
  return <EmptyState.Root icon="folder">
    <EmptyState.Title>{t(state === 'error' ? 'error.serverTitle' : state === 'missing' ? 'error.libraryNotFound' : 'browse.otherEmpty')}</EmptyState.Title>
    {retry ? <EmptyState.Actions><Button size="sm" variant="outline" label={t('error.retry')} onPress={retry} /></EmptyState.Actions> : null}
  </EmptyState.Root>;
}

function Photos({ library, viewer }: Scope) {
  const { queries } = useWebRuntime();
  const { photo } = Route.useSearch();
  const navigate = Route.useNavigate();
  const q = useQuery(queries.catalogQueries.photos(library.id, undefined, viewer));
  if (q.isError) return <Notice state="error" retry={() => void q.refetch()} />;
  if (q.isPending) return <SkeletonRow count={8} />;
  return q.data.length ? <PhotoGrid photos={q.data} photo={photo} onSelect={id => void navigate({ search: id ? { photo: id } : {}, replace: true, resetScroll: false })} /> : <Notice state="empty" />;
}

function Other({ library, viewer }: Scope) {
  const { queries } = useWebRuntime();
  const q = useQuery(queries.catalogQueries.other(library.id, viewer));
  if (q.isError) return <Notice state="error" retry={() => void q.refetch()} />;
  if (q.isPending) return <SkeletonRow count={8} />;
  return q.data.length ? <Box gap={10} mt={24}>{q.data.map((item) => <OtherRow key={item.id} item={item} />)}</Box> : <Notice state="empty" />;
}

function Titles({ library, viewer }: Scope) {
  const { queries } = useWebRuntime();
  const films = library.kind !== 'shows';
  const series = library.kind !== 'movies';
  const movies = useQuery({ ...queries.catalogQueries.moviesView(library.id, viewer), enabled: films });
  const shows = useQuery({ ...queries.catalogQueries.showsView(library.id, viewer), enabled: series });
  const entries = useMemo(() => {
    const list: CatalogEntry[] = [
      ...(films && movies.data ? movies.data.map((movie) => ({ kind: 'movie' as const, movie })) : []),
      ...(series && shows.data ? shows.data.map((show) => ({ kind: 'show' as const, show })) : []),
    ];
    const compare = compareTitles('added');
    return list.sort((a, b) => compare(a.kind === 'movie' ? a.movie : a.show, b.kind === 'movie' ? b.movie : b.show));
  }, [films, series, movies.data, shows.data]);
  if ((films && movies.isError) || (series && shows.isError)) {
    return <Notice state="error" retry={() => {
      if (films && movies.isError) void movies.refetch();
      if (series && shows.isError) void shows.refetch();
    }} />;
  }
  if ((films && movies.isPending) || (series && shows.isPending)) return <SkeletonRow count={8} />;
  return entries.length ? <CatalogGrid entries={entries} /> : <Notice state="empty" />;
}
