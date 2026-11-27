import { LibraryId, type MediaItem } from '@kromatv/client/media';
import { useT } from '@kromatv/ui';
import { Box, Chip, EmptyState, PageHeader, Row, Surface, Text } from '@kromatv/ui/kit';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { createFileRoute } from '@tanstack/react-router';
import { useWebRuntime } from '#web/shared/lib/web-runtime';
import { PageFrame, SkeletonRow } from '#web/shared/ui';
import { RouteLink } from '#web/shared/ui/route-link.component';

export const Route = createFileRoute('/_app/other')({
  validateSearch: (raw: Record<string, unknown>) => ({
    library: typeof raw.library === 'string' ? raw.library : undefined,
  }),
  component: OtherPage,
});

export function OtherRow({ item }: Readonly<{ item: MediaItem }>) {
  const t = useT();
  return (
    <Surface tone="outline" pad="sm">
      <Row between gap={16} align="center" wrap>
        <Box minW={0} flex>
          <Text variant="label" numberOfLines={2}>{item.title}</Text>
          <Text variant="meta" color="textDim" mt={4}>
            {[item.container.toUpperCase(), item.durationMs ? `${Math.round(item.durationMs / 60000)} min` : '']
              .filter(Boolean)
              .join(' · ')}
          </Text>
        </Box>
        <RouteLink to="/watch/$id" params={{ id: item.id }}>
          <Text variant="label" color="accentText">{t('content.play')}</Text>
        </RouteLink>
      </Row>
    </Surface>
  );
}

function OtherPage() {
  const t = useT();
  const runtime = useWebRuntime();
  const search = Route.useSearch();
  const navigate = Route.useNavigate();
  const library = search.library ? LibraryId.parse(search.library) : undefined;
  const { data: libraries = [] } = useQuery(runtime.queries.catalogQueries.libraries());
  const { data: items, isPending } = useQuery({
    ...runtime.queries.catalogQueries.other(library),
    placeholderData: keepPreviousData,
  });
  return (
    <PageFrame>
      <PageHeader.Root>
        <PageHeader.Title suffix={items?.length ? String(items.length) : undefined}>
          {t('nav.other')}
        </PageHeader.Title>
        <PageHeader.Subtitle>{t('browse.otherHint')}</PageHeader.Subtitle>
      </PageHeader.Root>
      <Row wrap gap={8} mt={20} mb={18}>
        <Chip
          active={!search.library}
          variant="subtle"
          icon="library"
          label={t('browse.allLibraries')}
          onPress={() => navigate({ search: (prev) => ({ ...prev, library: undefined }) })}
        />
        {libraries.filter((lib) => lib.kind === 'other').map((lib) => (
          <Chip
            key={lib.id}
            active={lib.id === search.library}
            variant="subtle"
            label={lib.name}
            onPress={() => navigate({ search: (prev) => ({ ...prev, library: lib.id }) })}
          />
        ))}
      </Row>
      {isPending ? (
        <Box mt={24}><SkeletonRow count={8} /></Box>
      ) : items?.length ? (
        <Box gap={10} mt={24}>{items.map((item) => <OtherRow key={item.id} item={item} />)}</Box>
      ) : (
        <Box mt={24}>
          <EmptyState.Root icon="folder">
            <EmptyState.Title>{t('browse.otherEmpty')}</EmptyState.Title>
            <EmptyState.Hint>{t('browse.otherHint')}</EmptyState.Hint>
          </EmptyState.Root>
        </Box>
      )}
    </PageFrame>
  );
}
