import { LibraryId } from '@kromatv/client/media';
import { useT } from '@kromatv/ui';
import { Box } from '@kromatv/ui/kit';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { createFileRoute } from '@tanstack/react-router';
import { BrowseTitle } from '#web/features/catalog/browse-hero.component';
import { BrowseScreen } from '#web/features/catalog/browse-screen.component';
import { validateBrowseSearch } from '#web/features/catalog/browse-search';
import { MovieGrid } from '#web/features/catalog/cards.component';
import { isAuthed } from '#web/shared/lib/api';
import { useWebRuntime } from '#web/shared/lib/web-runtime';

import { PageFrame, SkeletonRow } from '#web/shared/ui';

export const Route = createFileRoute('/_app/movies/')({
  validateSearch: validateBrowseSearch,
  loader: async ({ context: { runtime: webRuntime, queryClient } }) => {
    if (!isAuthed()) {
      return;
    }
    await Promise.all([
      queryClient.ensureQueryData(webRuntime.queries.catalogQueries.moviesView()),
      queryClient.ensureQueryData(webRuntime.queries.catalogQueries.libraries()),
    ]);
  },
  pendingComponent: MoviesPending,
  component: MoviesPage,
});

function MoviesPending() {
  const t = useT();
  return (
    <PageFrame>
      <BrowseTitle>{t('nav.films')}</BrowseTitle>
      <Box mt={24}>
        <SkeletonRow count={14} />
      </Box>
    </PageFrame>
  );
}

function MoviesPage() {
  const webRuntime = useWebRuntime();

  const t = useT();
  const { sort = 'added', genre, library } = Route.useSearch();
  const navigate = Route.useNavigate();
  const lib = library ? LibraryId.parse(library) : undefined;
  const { data: libraries = [] } = useQuery(webRuntime.queries.catalogQueries.libraries());
  const { data: movies } = useQuery({
    ...webRuntime.queries.catalogQueries.moviesView(lib),
    placeholderData: keepPreviousData,
  });

  if (!movies) {
    return <MoviesPending />;
  }

  return (
    <BrowseScreen
      heading={t('nav.films')}
      items={movies}
      countKey="browse.count.movies"
      emptyIcon="movie"
      emptyTitle={t('content.filmsEmpty')}
      sort={sort}
      genre={genre}
      libraries={libraries}
      library={library}
      onSort={(mode, opts) =>
        navigate({ search: (p) => ({ ...p, sort: mode }), resetScroll: opts?.resetScroll })
      }
      onGenre={(g) => navigate({ search: (p) => ({ ...p, genre: g }) })}
      onLibrary={(id) => navigate({ search: (p) => ({ ...p, library: id }) })}
      renderGrid={(view) => <MovieGrid movies={view} />}
    />
  );
}
