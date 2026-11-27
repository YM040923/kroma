import { useT } from '@kromatv/ui';
import { useSuspenseQuery } from '@tanstack/react-query';
import { createFileRoute, redirect } from '@tanstack/react-router';
import { browseDestination } from '#web/features/catalog/browse-search';
import { BrowseScreen } from '#web/features/catalog/browse-screen.component';
import { MovieGrid } from '#web/features/catalog/cards.component';
import { isAuthed } from '#web/shared/lib/api';
import { useWebRuntime } from '#web/shared/lib/web-runtime';
import { PageFrame, SkeletonRow } from '#web/shared/ui';

export const Route = createFileRoute('/_app/movies/')({
  beforeLoad: ({ location }) => {
    const destination = browseDestination(location.search);
    if (destination) throw redirect({ ...destination, replace: true });
  },
  loader: async ({ context: { runtime, queryClient } }) => {
    if (isAuthed()) await queryClient.ensureQueryData(runtime.queries.catalogQueries.moviesView());
  },
  pendingComponent: () => <PageFrame><SkeletonRow count={14} /></PageFrame>,
  component: MoviesPage,
});

function MoviesPage() {
  const t = useT();
  const { queries } = useWebRuntime();
  const { data } = useSuspenseQuery(queries.catalogQueries.moviesView());
  return <BrowseScreen heading={t('nav.films')} items={data} countKey="browse.count.movies"
    emptyIcon="movie" emptyTitle={t('content.filmsEmpty')} renderGrid={(movies) => <MovieGrid movies={movies} />} />;
}
