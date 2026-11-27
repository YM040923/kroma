import { loadSession } from '@kromatv/client/accounts';
import { ItemId } from '@kromatv/client/media';
import { Box, styles, useStyles } from '@kromatv/ui/kit';
import { useQuery, useSuspenseQuery } from '@tanstack/react-query';
import { createFileRoute, redirect, useNavigate } from '@tanstack/react-router';
import { Player } from '#web/features/playback/player.component';
import { toMovieView } from '#web/shared/lib/api';
import { useWebRuntime } from '#web/shared/lib/web-runtime';

const sDefinition = styles({ black: { position: 'fixed', inset: 0, bg: 'black' } });
export const Route = createFileRoute('/_app/_player/watch/$id')({
  loader: async ({ cause, preload, params, context: { runtime: webRuntime, queryClient } }) => {
    const session = loadSession();
    if (!session) {
      throw redirect({ to: '/' });
    }
    const id = ItemId.parse(params.id);
    const { catalogQueries } = webRuntime.queries;
    if (!preload && cause !== 'stay') {
      void queryClient.prefetchQuery(catalogQueries.resumeProgress(id, session.user.id));
    }
    const item = await queryClient.fetchQuery(catalogQueries.item(id));
    if (item.kind === 'episode') {
      void queryClient.prefetchQuery(catalogQueries.following(id));
    }
  },

  pendingMs: 0,
  pendingMinMs: 0,
  pendingComponent: PendingPlayer,
  component: WatchPage,
});

function WatchPage() {
  const webRuntime = useWebRuntime();

  const id = ItemId.parse(Route.useParams().id);
  const { data: raw } = useSuspenseQuery(webRuntime.queries.catalogQueries.item(id));
  const item = toMovieView(webRuntime.client, raw);
  const { data: following = [] } = useQuery({
    ...webRuntime.queries.catalogQueries.following(id),
    enabled: item.kind === 'episode',
  });
  const next = following[0] ?? null;
  const navigate = useNavigate();
  return (
    <Player
      key={item.id}
      item={item}
      next={next}
      following={following}
      onPlayNext={next ? () => navigate({ to: '/watch/$id', params: { id: next.id } }) : undefined}
      onPlayItem={(itemId) => navigate({ to: '/watch/$id', params: { id: itemId } })}
      onGoHome={() => navigate({ to: '/', replace: true })}
      // Back returns to the detail page of what was playing: the series page for an
      // episode, otherwise the movie page (mirrors the catalog cards' deep-link rule).
      onClose={() =>
        item.kind === 'episode' && item.showId
          ? navigate({ to: '/shows/$id', params: { id: item.showId }, replace: true })
          : navigate({ to: '/movies/$id', params: { id: item.id }, replace: true })
      }
    />
  );
}

function PendingPlayer() {
  const s = useStyles(sDefinition);
  return <Box style={s.black} />;
}
