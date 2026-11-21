// Subscriptions as the centre draws them: one card per followed title, read from
// the subscriptions module's own admin API. The module keeps the data and the
// full editing page; this is the shelf, plus the one button that opens it.

import { useT } from '@kromatv/ui';
import { Badge, Button, Column, EmptyState, Img, Row, Text } from '@kromatv/ui/kit';
import { useCallback, useEffect, useState } from 'react';
import { apiBase } from '#web/shared/lib/api';
import { useAuth } from '#web/shared/lib/auth';
import { TableSkeleton } from '#web/shared/ui';

const MODULE = 'tv.kroma.subscriptions';

interface SubscriptionCard {
  id: string;
  name: string;
  poster: string;
  year: number;
  kind: string;
  status: string;
  lastError: string;
  season: number;
  episodeStart: number;
  episodeTotal: number;
  link: string;
  tmdbId: number;
}

/** `{ tasks, items, sourceUrl, ... }` is what the module's list route answers;
 *  only the cards are read here. */
interface SubscriptionView {
  tasks?: SubscriptionCard[];
  sourceUrl?: string;
}

async function readSubscriptions(token: string | undefined): Promise<SubscriptionView> {
  const response = await fetch(`${apiBase()}/api/admin/m/${MODULE}/subscriptions`, {
    headers: token ? { Authorization: `Bearer ${token}` } : {},
  });
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return (await response.json()) as SubscriptionView;
}

const STATE_TONE: Record<string, 'info' | 'warning' | 'danger' | 'success'> = {
  done: 'success',
  running: 'info',
  unlocking: 'info',
  transferring: 'info',
  pending: 'warning',
  failed: 'danger',
};

export function SubscriptionGrid({ onOpen }: Readonly<{ onOpen: () => void }>) {
  const t = useT();
  const { client } = useAuth();
  const [view, setView] = useState<SubscriptionView | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    setLoading(true);
    readSubscriptions(client.sessionToken)
      .then((reply) => {
        setView(reply);
        setError(null);
      })
      .catch((reason) => setError(String(reason)))
      .finally(() => setLoading(false));
  }, [client.sessionToken]);

  useEffect(load, [load]);

  if (loading && !view) {
    return <TableSkeleton rows={3} />;
  }
  if (error) {
    return (
      <EmptyState.Root>
        <EmptyState.Title>{t('centre.subsUnavailable')}</EmptyState.Title>
        <EmptyState.Hint>{error}</EmptyState.Hint>
      </EmptyState.Root>
    );
  }
  const cards = view?.tasks ?? [];
  if (cards.length === 0) {
    return (
      <EmptyState.Root>
        <EmptyState.Title>{t('centre.subsEmpty')}</EmptyState.Title>
        <EmptyState.Hint>{t('centre.subsEmptyHint')}</EmptyState.Hint>
        <EmptyState.Actions>
          <Button variant="primary" label={t('centre.subsOpen')} onPress={onOpen} />
        </EmptyState.Actions>
      </EmptyState.Root>
    );
  }
  return (
    <Column gap={12}>
      <Row gap={8} center wrap>
        <Text variant="meta" color="textMuted">
          {t('centre.subsCount', { count: cards.length })}
        </Text>
        <Button size="sm" variant="glass" label={t('centre.subsOpen')} onPress={onOpen} />
      </Row>
      <Row gap={12} wrap>
        {cards.map((card) => (
          <Column key={card.id} gap={6} style={{ width: 140 }}>
            <Img src={card.poster} style={{ width: 140, height: 210, borderRadius: 8 }} />
            <Text variant="body" lines={2}>
              {card.name}
            </Text>
            <Row gap={6} center wrap>
              <Badge tone={STATE_TONE[card.status] ?? 'neutral'}>{card.status}</Badge>
              {card.year ? (
                <Text variant="meta" color="textMuted">
                  {card.year}
                </Text>
              ) : null}
            </Row>
            {card.lastError ? (
              <Text variant="meta" color="danger" lines={2}>
                {card.lastError}
              </Text>
            ) : null}
          </Column>
        ))}
      </Row>
    </Column>
  );
}