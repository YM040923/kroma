import {
  type CollectionView,
  type MediaCard,
  type MediaItem,
  ItemId,
} from '@kromatv/client/media';
import { useT } from '@kromatv/ui';
import { useQuery, useSuspenseQuery } from '@tanstack/react-query';
import { createFileRoute, redirect } from '@tanstack/react-router';
import { TitleDetail } from '#web/features/catalog/title-detail.component';
import { isAuthed } from '#web/shared/lib/api';
import { useAuth } from '#web/shared/lib/auth';
import { buildTitleView, movieSimilarTargets } from '#web/shared/lib/title-view';
import { useCatalogLiveRefresh } from '#web/shared/lib/use-live-refresh.hook';
import { useWebRuntime } from '#web/shared/lib/web-runtime';
import { DetailSkeleton } from '#web/shared/ui';

const SIMILAR_COUNT = 12;

export const Route = createFileRoute('/_app/movies/$id')({
  loader: async ({ params, context: { runtime: webRuntime, queryClient } }) => {
    // Catalogue is auth-gated: a signed-out deep-link goes to the gate (the
    // AuthGate overlay covers /, and its loader is guarded too).
    if (!isAuthed()) {
      throw redirect({ to: '/' });
    }
    const id = ItemId.parse(params.id);
    const { catalogQueries } = webRuntime.queries;
    void queryClient.prefetchQuery(catalogQueries.similar(id));
    void queryClient.prefetchQuery(catalogQueries.collection(id));
    await queryClient.ensureQueryData(catalogQueries.item(id));
  },
  pendingComponent: DetailSkeleton,
  component: MovieDetailPage,
});

function MovieDetailPage() {
  const webRuntime = useWebRuntime();

  const id = ItemId.parse(Route.useParams().id);
  // Re-enrichment (incl. a corrected TMDB match) lands via the event stream.
  useCatalogLiveRefresh('item', id);
  const { data: item } = useSuspenseQuery(webRuntime.queries.catalogQueries.item(id));
  const { data: similar } = useQuery(webRuntime.queries.catalogQueries.similar(id));
  const { data: collection } = useQuery(webRuntime.queries.catalogQueries.collection(id));
  return (
    <MovieDetail
      item={item}
      similar={similar?.slice(0, SIMILAR_COUNT)}
      collection={collection}
    />
  );
}

function MovieDetail({
  item,
  similar,
  collection,
}: Readonly<{
  item: MediaItem;
  similar: MediaCard[] | undefined;
  collection: CollectionView | undefined;
}>) {
  const t = useT();
  const { client, user } = useAuth();
  const view = buildTitleView(client, t, user, {
    source: 'movie',
    item,
    similar: [],
    discover: null,
  });
  return (
    <TitleDetail
      key={item.id}
      initial={view}
      similar={similar && movieSimilarTargets(client, t, similar)}
      collection={
        collection?.items.length
          ? {
              name: collection.name ?? null,
              targets: movieSimilarTargets(client, t, collection.items),
            }
          : null
      }
    />
  );
}
