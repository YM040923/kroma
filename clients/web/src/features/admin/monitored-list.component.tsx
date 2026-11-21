// The titles being followed: a compact list for the acquisition centre, where
// the question is "what is watched" rather than "what is in the library". The
// full library index stays on the library page.

import { useT } from '@kromatv/ui';
import { Button, Column, EmptyState, Row, Text } from '@kromatv/ui/kit';
import { useState } from 'react';
import { useMonitoredTitles } from '#web/features/admin/use-monitoring';
import { useAuth } from '#web/shared/lib/auth';
import { TableSkeleton } from '#web/shared/ui';

export function MonitoredList() {
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
    return <TableSkeleton rows={4} />;
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
    <Column gap={4}>
      {failed ? (
        <Text variant="meta" color="danger">
          {t('centre.followFailed')}
        </Text>
      ) : null}
      {titles.map((row) => {
        const key = `${row.kind}:${row.tmdbId}`;
        return (
          <Row key={key} gap={12} center>
            <Column flex gap={2}>
              <Text variant="body">{row.title}</Text>
              <Text variant="meta" color="textMuted">
                {[
                  row.year ? String(row.year) : null,
                  row.kind === 'show' ? t('centre.kindShow') : t('centre.kindMovie'),
                ]
                  .filter(Boolean)
                  .join(' · ')}
              </Text>
            </Column>
            <Button
              size="sm"
              variant="ghost"
              label={t('centre.unfollow')}
              loading={busy.has(key)}
              onPress={() => unfollow(row.kind, row.tmdbId)}
            />
          </Row>
        );
      })}
    </Column>
  );
}