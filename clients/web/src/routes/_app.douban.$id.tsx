import type { DoubanPerson } from '@kromatv/client/douban';
import { posterColors } from '@kromatv/core';
import { useT } from '@kromatv/ui';
import { Box, Button, EmptyState, Text } from '@kromatv/ui/kit';
import { useQuery } from '@tanstack/react-query';
import { createFileRoute } from '@tanstack/react-router';
import type { CSSProperties } from 'react';
import { DetailHero } from '#web/features/catalog/detail.component';
import { imageUrl } from '#web/shared/lib/api';
import { useAuth } from '#web/shared/lib/auth';
import { Image, PageFrame, SkeletonRow } from '#web/shared/ui';
import { RouteLink } from '#web/shared/ui/route-link.component';

// One Douban subject, the same hero the library's own fiches draw. Nothing
// plays or gets requested from here: the id is Douban's, not the library's.
export const Route = createFileRoute('/_app/douban/$id')({
  component: DoubanDetailPage,
});

const GUTTER: CSSProperties = {
  paddingLeft: 'var(--gutter-web)',
  paddingRight: 'var(--gutter-web)',
};

const RAIL: CSSProperties = {
  display: 'flex',
  gap: 22,
  overflowX: 'auto',
  paddingTop: 8,
  paddingBottom: 8,
};

const FACE = { width: 96, height: 96, borderRadius: 48 } as const;

function DoubanDetailPage() {
  const t = useT();
  const { client } = useAuth();
  const id = Route.useParams().id;
  const query = useQuery({
    queryKey: ['douban', 'subject', id],
    queryFn: () => client.douban.subject(id),
    staleTime: 24 * 60 * 60_000,
    retry: 0,
  });

  if (query.isPending) {
    return (
      <PageFrame>
        <Box mt={24}>
          <SkeletonRow count={6} />
        </Box>
      </PageFrame>
    );
  }
  if (query.isError || !query.data) {
    return (
      <PageFrame>
        <EmptyState.Root icon="star">
          <EmptyState.Title>{t('browse.doubanEmpty')}</EmptyState.Title>
          <EmptyState.Hint>{t('browse.doubanHint')}</EmptyState.Hint>
        </EmptyState.Root>
      </PageFrame>
    );
  }

  const detail = query.data;
  const card = detail.card;
  const meta = [card.year, detail.country, detail.language].filter(Boolean).join(' · ');

  return (
    <main style={{ minWidth: 0, paddingBottom: 80 }}>
      <DetailHero
        art={{
          id: card.id,
          backdrop: imageUrl(client.douban.imageUrl(card.backdropUrl)),
          poster: imageUrl(client.douban.imageUrl(card.posterUrl)) ?? '',
        }}
        overline={card.genres.slice(0, 3).join(' · ') || '豆瓣'}
        title={card.title}
        rating={card.rating > 0 ? card.rating : null}
        meta={meta}
        badges={[]}
        overview={card.overview || null}
        back={<RouteLink to="/douban" />}
        primaryAction={
          detail.sourceUrl ? (
            <Button label={t('browse.doubanOpen')} icon="external-link" variant="glass" size="sm" asChild>
              <a href={detail.sourceUrl} target="_blank" rel="noreferrer" />
            </Button>
          ) : null
        }
      />
      <div style={GUTTER}>
        {detail.released ? (
          <Text variant="meta" color="textDim" mt={20}>
            {detail.released}
          </Text>
        ) : null}
        {detail.aliases.length ? (
          <Text variant="meta" color="textDim" mt={4}>
            {detail.aliases.join(' / ')}
          </Text>
        ) : null}
        {detail.directors.length ? (
          <Text variant="meta" color="textDim" mt={8}>
            {t('content.directedBy')} · {detail.directors.map((person) => person.name).join(' / ')}
          </Text>
        ) : null}
      </div>
      <PeopleRow title={t('content.cast')} people={detail.cast} />
    </main>
  );
}

function PeopleRow({ title, people }: Readonly<{ title: string; people: DoubanPerson[] }>) {
  if (people.length === 0) {
    return null;
  }
  return (
    <section style={{ ...GUTTER, marginTop: 40 }}>
      <Text variant="h2" mb={16}>
        {title}
      </Text>
      <div style={RAIL}>
        {people.map((person) => (
          <PersonTile key={`${person.name}-${person.role ?? ''}`} person={person} />
        ))}
      </div>
    </section>
  );
}

function PersonTile({ person }: Readonly<{ person: DoubanPerson }>) {
  const { client } = useAuth();
  const [c1, c2] = posterColors(person.name);
  const avatar = imageUrl(client.douban.imageUrl(person.avatarUrl));
  const face = (
    <div
      style={{
        width: '100%',
        height: '100%',
        display: 'grid',
        placeItems: 'center',
        background: `linear-gradient(135deg, ${c1}, ${c2})`,
      }}
    >
      <Text variant="h2" color="white/90">
        {person.name.slice(0, 1)}
      </Text>
    </div>
  );
  return (
    <Box align="center" w={96} shrink={0}>
      <Image style={FACE} src={avatar} alt={person.name} placeholder={face} fallback={face} />
      <Text variant="label" mt={8} textAlign="center" lines={1}>
        {person.name}
      </Text>
      {person.role ? (
        <Text variant="meta" color="textDim" textAlign="center" lines={1}>
          {person.role}
        </Text>
      ) : null}
    </Box>
  );
}
