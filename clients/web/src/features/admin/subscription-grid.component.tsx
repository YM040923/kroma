// Subscriptions, managed here rather than on a second page: the same module
// routes the old page called, driven by a grid of posters. A subscription's only
// jobs are "watch this share for me" and "take what arrives", so that is all
// this panel does.

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
  resource: string;
  followingShares?: { shareCode: string; name: string; seen?: string[] }[];
  sentEpisodes?: string[];
}

interface SubscriptionView {
  tasks?: SubscriptionCard[];
  sourceUrl?: string;
}

async function moduleCall<T>(
  path: string,
  token: string | undefined,
  init?: { method?: string; body?: unknown },
): Promise<T> {
  const response = await fetch(`${apiBase()}/api/admin/m/${MODULE}${path}`, {
    method: init?.method ?? 'GET',
    headers: {
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      ...(init?.body ? { 'Content-Type': 'application/json' } : {}),
    },
    body: init?.body ? JSON.stringify(init.body) : undefined,
  });
  if (!response.ok) {
    const text = await response.text().catch(() => '');
    throw new Error(text.trim().slice(0, 200) || `HTTP ${response.status}`);
  }
  return (await response.json()) as T;
}

const TONE: Record<string, 'info' | 'warning' | 'danger' | 'success' | 'neutral'> = {
  done: 'success',
  running: 'info',
  unlocking: 'info',
  transferring: 'info',
  pending: 'warning',
  failed: 'danger',
};

export function SubscriptionGrid() {
  const t = useT();
  const { client } = useAuth();
  const [view, setView] = useState<SubscriptionView | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const load = useCallback(() => {
    setLoading(true);
    moduleCall<SubscriptionView>('/subscriptions', client.sessionToken)
      .then((reply) => {
        setView(reply);
        setError(null);
      })
      .catch((reason) => setError(String(reason)))
      .finally(() => setLoading(false));
  }, [client.sessionToken]);

  useEffect(load, [load]);

  const act = (id: string, path: string, method: 'POST' | 'PUT') => {
    setBusyId(id);
    setNotice(null);
    moduleCall(`/subscriptions/${id}${path}`, client.sessionToken, { method, body: {} })
      .then(() => {
        setNotice(t('centre.subsActionDone'));
        load();
      })
      .catch((reason) => setNotice(String(reason)))
      .finally(() => setBusyId(null));
  };

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
      </EmptyState.Root>
    );
  }
  return (
    <Column gap={12}>
      <Row gap={8} center wrap>
        <Text variant="meta" color="textMuted">
          {t('centre.subsCount', { count: cards.length })}
        </Text>
        {notice ? (
          <Text variant="meta" color="textMuted">
            {notice}
          </Text>
        ) : null}
      </Row>
      <Row gap={12} wrap>
        {cards.map((card) => {
          const shares = card.followingShares ?? [];
          return (
            <Column key={card.id} gap={6} style={{ width: 150 }}>
              <Img src={card.poster} style={{ width: 150, height: 225, borderRadius: 8 }} />
              <Text variant="body" lines={2}>
                {card.name}
              </Text>
              <Row gap={6} center wrap>
                <Badge tone={TONE[card.status] ?? 'neutral'}>{card.status}</Badge>
                {card.year ? (
                  <Text variant="meta" color="textMuted">
                    {card.year}
                  </Text>
                ) : null}
              </Row>
              {shares.length > 0 ? (
                <Text variant="meta" color="textMuted" lines={1}>
                  {t('centre.subsFollowing', { count: shares.length })}
                </Text>
              ) : null}
              {card.lastError ? (
                <Text variant="meta" color="danger" lines={2}>
                  {card.lastError}
                </Text>
              ) : null}
              {/* A follow-only task has no release of its own: its first
                  transfer already happened through the acquisition panel, so
                  there is nothing here to grab and nothing to pay twice for. */}
              {card.resource ? (
                <Button
                  size="sm"
                  variant="glass"
                  label={t('centre.subsTakeNow')}
                  loading={busyId === card.id}
                  onPress={() => act(card.id, '/execute', 'POST')}
                />
              ) : (
                <Text variant="meta" color="textMuted" lines={1}>
                  {t('centre.subsFollowingOnly')}
                </Text>
              )}
            </Column>
          );
        })}
      </Row>
    </Column>
  );
}