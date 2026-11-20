import type { DoubanCard, DoubanKind, DoubanRowStub } from '@kromatv/client/douban';
import { posterColors } from '@kromatv/core';
import { useT } from '@kromatv/ui';
import { Box, Chip, EmptyState, PageHeader, Row, Text } from '@kromatv/ui/kit';
import { useQuery } from '@tanstack/react-query';
import { createFileRoute } from '@tanstack/react-router';
import { imageUrl } from '#web/shared/lib/api';
import { useAuth } from '#web/shared/lib/auth';
import { PageFrame, Poster, PosterRail, SkeletonRow } from '#web/shared/ui';
import { RouteLink } from '#web/shared/ui/route-link.component';

// Douban's ranked rows, read-only: its own mobile pages are the source, its
// ids never become library ids. A row that fails collapses; the rest of the
// page stays.
export const Route = createFileRoute('/_app/douban')({
  validateSearch: (raw: Record<string, unknown>): { kind?: DoubanKind } => ({
    kind: raw.kind === 'tv' ? 'tv' : 'movie',
  }),
  component: DoubanPage,
});

const ROW_STALE = 6 * 60 * 60_000;

function DoubanPage() {
  const t = useT();
  const { client } = useAuth();
  const kind = Route.useSearch().kind ?? 'movie';
  const navigate = Route.useNavigate();
  const layout = useQuery({
    queryKey: ['douban', 'layout', kind],
    queryFn: () => client.douban.layout(kind),
  });

  const pick = (next: DoubanKind) => {
    void navigate({ search: (prev) => ({ ...prev, kind: next }), replace: true });
  };

  return (
    <PageFrame>
      <PageHeader.Root>
        <PageHeader.Title>{t('nav.douban')}</PageHeader.Title>
        <PageHeader.Subtitle>{t('browse.doubanHint')}</PageHeader.Subtitle>
      </PageHeader.Root>
      <Row wrap gap={8} mt={20} mb={18}>
        <Chip
          active={kind === 'movie'}
          variant="subtle"
          label={t('browse.doubanMovies')}
          onPress={() => pick('movie')}
        />
        <Chip
          active={kind === 'tv'}
          variant="subtle"
          label={t('browse.doubanTv')}
          onPress={() => pick('tv')}
        />
      </Row>
      {layout.isPending ? (
        <Box mt={24}>
          <SkeletonRow count={8} />
        </Box>
      ) : null}
      {layout.isError ? (
        <EmptyState.Root icon="star">
          <EmptyState.Title>{t('browse.doubanEmpty')}</EmptyState.Title>
          <EmptyState.Hint>{t('browse.doubanHint')}</EmptyState.Hint>
        </EmptyState.Root>
      ) : null}
      {layout.data?.rows.map((stub) => (
        <DoubanRow key={stub.id} kind={kind} stub={stub} />
      ))}
    </PageFrame>
  );
}

function DoubanRow({ kind, stub }: Readonly<{ kind: DoubanKind; stub: DoubanRowStub }>) {
  const { client } = useAuth();
  const query = useQuery({
    queryKey: ['douban', 'row', kind, stub.id],
    queryFn: () => client.douban.row(kind, stub.id),
    staleTime: ROW_STALE,
    retry: 0,
  });

  if (query.isPending) {
    return (
      <Box mb={30}>
        <Text variant="h2" mb={16}>
          {stub.title}
        </Text>
        <SkeletonRow count={8} />
      </Box>
    );
  }
  if (!query.data || query.data.items.length === 0) {
    return null;
  }
  return (
    <Box mb={30}>
      <Text variant="h2" mb={4}>
        {stub.title}
      </Text>
      <PosterRail
        data={query.data.items}
        renderItem={(card) => <DoubanPoster card={card} />}
      />
    </Box>
  );
}

function DoubanPoster({ card }: Readonly<{ card: DoubanCard }>) {
  const { client } = useAuth();
  const poster = imageUrl(client.douban.imageUrl(card.posterUrl));
  const caption = [card.rating > 0 ? `${card.rating.toFixed(1)}★` : null, card.genres[0]]
    .filter(Boolean)
    .join(' · ');
  return (
    <Poster
      title={card.title}
      genre={caption || undefined}
      colors={posterColors(card.id)}
      poster={poster}
      asChild
    >
      <RouteLink to="/douban/$id" params={{ id: card.id }} />
    </Poster>
  );
}
