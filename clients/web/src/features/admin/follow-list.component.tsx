// What is being followed: one card per monitored title, with its poster. The
// library index is the place to browse everything; this list only answers
// "what will the server look for on its own".

import { useT } from '@kromatv/ui';
import { Badge, Button, EmptyState, Img, Text } from '@kromatv/ui/kit';
import { Column, Row } from '@kromatv/ui/kit';
import { useState } from 'react';
import { useMonitoredTitles } from '#web/features/admin/use-monitoring';
import { useAuth } from '#web/shared/lib/auth';
import { TableSkeleton } from '#web/shared/ui';

export function FollowList() {
  const t = useT();
  const { client } = useAuth();
  const { data, loading, reload } = useMonitoredTitles();
  const [busy, setBusy] = useState<ReadonlySet<string>>(() => new Set());
  const [failed, setFailed] = useState(false);

  const unfollow = (kind: 'movie' | 'show', tmdbId: number) => {
    const key = `${kind}:${tmdbId}`;
    setBusy((set) => new Set(set).add(key));
    setFailed(false);
    client.monitoring
      .unmonitor(kind, tmdbId)
      .then(() => reload())
      .catch(() => setFailed(true))
      .finally(() =>
        setBusy((set) => {
          const next = new Set(set);
          next.delete(key);
          return next;
        }),
      );
  };

  if (loading && !data) {
    return <TableSkeleton rows={3} />;
  }
  const titles = data ?? [];
  if (titles.length === 0) {
    return (
      <EmptyState.Root>
        <EmptyState.Title>{t('centre.followEmpty')}</EmptyState.Title>
        <EmptyState.Hint>{t('centre.followEmptyHint')}</EmptyState.Hint>
      </EmptyState.Root>
    );
  }
  return (
    <Column gap={12}>
      <Text variant="meta" color="textMuted">
        {t('centre.followHint')}
      </Text>
      {failed ? (
        <Text variant="meta" color="danger">
          {t('centre.followFailed')}
        </Text>
      ) : null}
      <Row gap={12} wrap>
        {titles.map((row) => {
          const key = `${row.kind}:${row.tmdbId}`;
          return (
            <Column key={key} gap={6} style={{ width: 132 }}>
              <Img
                src={row.posterUrl ?? ''}
                style={{ width: 132, height: 198, borderRadius: 8 }}
              />
              <Text variant="body" lines={2}>
                {row.title}
              </Text>
              <Row gap={6} center wrap>
                <Badge tone="neutral">
                  {row.kind === 'show' ? t('centre.kindShow') : t('centre.kindMovie')}
                </Badge>
                {row.year ? (
                  <Text variant="meta" color="textMuted">
                    {row.year}
                  </Text>
                ) : null}
              </Row>
              <Button
                size="sm"
                variant="ghost"
                label={t('centre.unfollow')}
                loading={busy.has(key)}
                onPress={() => unfollow(row.kind, row.tmdbId)}
              />
            </Column>
          );
        })}
      </Row>
    </Column>
  );
}