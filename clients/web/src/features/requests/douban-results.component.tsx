import type { DoubanSearchItem } from '@kromatv/client/douban';
import { useT } from '@kromatv/ui';
import { EmptyState, Text } from '@kromatv/ui/kit';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import type { CSSProperties } from 'react';
import { imageUrl } from '#web/shared/lib/api';
import { useAuth } from '#web/shared/lib/auth';

// Douban's own search, inside the search page and beside TMDB. It has no page
// of its own: picking a title re-runs the search on TMDB, where a request can
// actually be made.

const GRID: CSSProperties = {
  display: 'grid',
  gridTemplateColumns: 'repeat(auto-fill, minmax(150px, 1fr))',
  gap: 18,
  padding: '0 var(--gutter-web) 40px',
};

const TILE: CSSProperties = {
  display: 'flex',
  flexDirection: 'column',
  alignItems: 'flex-start',
  background: 'none',
  border: 0,
  padding: 0,
  color: 'inherit',
  font: 'inherit',
  textAlign: 'left',
  cursor: 'pointer',
};

const POSTER: CSSProperties = {
  width: '100%',
  aspectRatio: '2 / 3',
  objectFit: 'cover',
  display: 'block',
  borderRadius: 14,
  background: 'var(--kroma-surface-1)',
};

export function DoubanResults({
  q,
  onPick,
}: Readonly<{ q: string; onPick: (title: string) => void }>) {
  const t = useT();
  const { client } = useAuth();
  const { data = [], isPending } = useQuery<DoubanSearchItem[]>({
    queryKey: ['douban', 'search', q],
    queryFn: () => client.douban.search(q),
    staleTime: 10 * 60_000,
    placeholderData: keepPreviousData,
  });

  if (isPending) {
    return (
      <div style={GRID}>
        <Text variant="meta" color="textDim">
          …
        </Text>
      </div>
    );
  }
  if (data.length === 0) {
    return (
      <EmptyState.Root icon="search">
        <EmptyState.Title>{t('discover.doubanEmpty')}</EmptyState.Title>
      </EmptyState.Root>
    );
  }
  return (
    <div style={GRID}>
      {data.map((item) => (
        <button
          key={item.id}
          type="button"
          style={TILE}
          onClick={() => onPick(item.title)}
        >
          <img
            src={imageUrl(client.douban.imageUrl(item.posterUrl)) ?? ''}
            alt={item.title}
            style={POSTER}
          />
          <Text variant="label" mt={8} lines={1}>
            {item.title}
          </Text>
          {item.rating > 0 ? (
            <Text variant="meta" color="textDim">
              {`${item.rating.toFixed(1)}★`}
            </Text>
          ) : null}
        </button>
      ))}
    </div>
  );
}
