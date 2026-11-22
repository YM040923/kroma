// The acquisition panel: the one place a release is listed, priced and taken.
// There is one source today (MADREEL), so there is no source switcher to click
// through — a tab that always says "not wired" is worse than no tab. Probing a
// price opens each candidate's page, which takes a while, so the wait says so.

import type { KromaClient } from '@kromatv/client';
import { apiErrorText } from '@kromatv/client';
import {
  type AcquisitionCandidate,
  type AcquisitionCandidates,
  candidatePayload,
} from '@kromatv/client/acquisition';
import type { Translate } from '@kromatv/i18n';
import { useT } from '@kromatv/ui';
import {
  Badge,
  Button,
  Column,
  Dialog,
  EmptyState,
  Row,
  Skeleton,
  Spacer,
  Text,
} from '@kromatv/ui/kit';
import { useCallback, useEffect, useState } from 'react';
import type { AcquisitionTarget } from '#web/shared/lib/use-acquisition.hook';

/** What the panel needs to ask for one title. */
export interface AcquirePanelTarget {
  target: AcquisitionTarget;
  /** The library the title aims at, when the page knows it. */
  library?: string | null;
  /** For a show: the season the panel lists. */
  season?: number | null;
}

interface PanelProps {
  open: boolean;
  onClose: () => void;
  client: KromaClient;
  ask: AcquirePanelTarget | null;
  /** What the caller does with the release it picked, or with the server's own
   *  choice (`null`) when the panel runs in automatic mode. */
  onTake: (candidate: Record<string, unknown> | null) => Promise<void>;
}

function bytesOf(n: number | null | undefined): string {
  if (!n || n <= 0) {
    return '';
  }
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let value = n;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value >= 10 || unit === 0 ? Math.round(value) : value.toFixed(1)} ${units[unit]}`;
}

function priceLabel(t: Translate, one: AcquisitionCandidate): string {
  const price = one.price;
  if (price == null) {
    return t('acquire.priceUnknown');
  }
  if (price === 0) {
    return t('acquire.priceFree');
  }
  return t('acquire.pricePoints', { points: price });
}

function fileNameOf(one: AcquisitionCandidate): string {
  return one.fileName || one.resource || one.shareCode || `#${one.index}`;
}

function CandidateRow({
  one,
  busy,
  onPick,
}: Readonly<{ one: AcquisitionCandidate; busy: boolean; onPick: () => void }>) {
  const t = useT();
  const size = bytesOf(one.sizeBytes ?? null) || one.size || '';
  return (
    <Row gap={12} center p={12} radius="md" bg="surface1">
      <Column flex gap={4}>
        <Row gap={6} center wrap>
          {one.recommended ? <Badge tone="info">{t('acquire.recommended')}</Badge> : null}
          {one.needsConfirm ? <Badge tone="warning">{t('acquire.needsConfirm')}</Badge> : null}
          <Text variant="body" lines={2}>
            {fileNameOf(one)}
          </Text>
        </Row>
        <Row gap={10} wrap>
          {size ? (
            <Text variant="meta" color="textMuted">
              {size}
            </Text>
          ) : null}
          <Text variant="meta" color={one.needsConfirm ? 'danger' : 'textMuted'}>
            {priceLabel(t, one)}
          </Text>
        </Row>
      </Column>
      <Spacer />
      <Button
        size="sm"
        variant={one.recommended ? 'primary' : 'glass'}
        label={t('acquire.take')}
        disabled={busy}
        onPress={onPick}
      />
    </Row>
  );
}

/** The list for one target: fetched once per open, taken row by row. */
function AcquireList({
  client,
  ask,
  onTake,
}: Readonly<{
  client: KromaClient;
  ask: AcquirePanelTarget;
  onTake: (candidate: Record<string, unknown>) => Promise<void>;
}>) {
  const t = useT();
  const [data, setData] = useState<AcquisitionCandidates | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [taking, setTaking] = useState(false);

  useEffect(() => {
    let live = true;
    setLoading(true);
    setError(null);
    client.acquisition
      .candidates({
        kind: ask.target.kind,
        tmdbId: ask.target.tmdbId,
        title: ask.target.title,
        year: ask.target.year,
        library: ask.library ?? null,
        season: ask.season ?? null,
      })
      .then((reply) => {
        if (live) {
          setData(reply);
        }
      })
      .catch((reason) => {
        if (live) {
          setError(apiErrorText(reason, t('acquire.searchFailed')));
        }
      })
      .finally(() => {
        if (live) {
          setLoading(false);
        }
      });
    return () => {
      live = false;
    };
  }, [ask.target.kind, ask.target.tmdbId, ask.target.title, ask.target.year, ask.library, ask.season, client, t]);

  const take = useCallback(
    async (candidate: Record<string, unknown>) => {
      setTaking(true);
      try {
        await onTake(candidate);
      } finally {
        setTaking(false);
      }
    },
    [onTake],
  );

  if (loading) {
    return (
      <Column gap={8}>
        <Text variant="meta" color="textMuted">
          {t('acquire.searching')}
        </Text>
        <Skeleton shape="block" style={{ height: 54 }} />
        <Skeleton shape="block" style={{ height: 54 }} />
        <Skeleton shape="block" style={{ height: 54 }} />
      </Column>
    );
  }

  if (error) {
    return (
      <EmptyState.Root>
        <EmptyState.Title>{t('acquire.searchFailed')}</EmptyState.Title>
        <EmptyState.Hint>{error}</EmptyState.Hint>
      </EmptyState.Root>
    );
  }

  if (data?.owned) {
    return (
      <EmptyState.Root>
        <EmptyState.Title>{t('acquire.alreadyHeld')}</EmptyState.Title>
        <EmptyState.Hint>{t('acquire.alreadyHeldBody')}</EmptyState.Hint>
      </EmptyState.Root>
    );
  }

  if (data && data.blockers.length > 0) {
    return (
      <EmptyState.Root>
        <EmptyState.Title>{t('acquire.notReady')}</EmptyState.Title>
        <EmptyState.Hint>{data.blockers.join(' · ')}</EmptyState.Hint>
      </EmptyState.Root>
    );
  }

  if (!data || data.candidates.length === 0) {
    return (
      <EmptyState.Root>
        <EmptyState.Title>{t('acquire.noRelease')}</EmptyState.Title>
        <EmptyState.Hint>{t('acquire.noReleaseBody')}</EmptyState.Hint>
      </EmptyState.Root>
    );
  }

  return (
    <Column gap={4}>
      {data.candidates.map((one) => (
        <CandidateRow
          key={one.id}
          one={one}
          busy={taking}
          onPick={() => void take(candidatePayload(one))}
        />
      ))}
    </Column>
  );
}

/** The one acquisition panel. Closing it never cancels work the server already
 *  started: the attempt keeps its own id and the tasks tab follows it. */
export function AcquirePanel({ open, onClose, client, ask, onTake }: Readonly<PanelProps>) {
  const t = useT();
  const [auto, setAuto] = useState(false);
  const [autoBusy, setAutoBusy] = useState(false);

  const take = useCallback(
    async (candidate: Record<string, unknown>) => {
      await onTake(candidate);
      onClose();
    },
    [onTake, onClose],
  );

  const runAuto = useCallback(async () => {
    setAutoBusy(true);
    try {
      await onTake(null);
      onClose();
    } finally {
      setAutoBusy(false);
    }
  }, [onTake, onClose]);

  return (
    <Dialog.Root open={open} onClose={onClose} title={t('acquire.title')} width="lg" fullHeight>
      <Dialog.Header>
        <Row gap={12} center wrap>
          <Column gap={2} flex>
            <Text variant="title">{t('acquire.title')}</Text>
            {ask ? (
              <Text variant="meta" color="textMuted">
                {ask.target.title}
                {ask.target.year ? ` (${ask.target.year})` : ''}
                {ask.season ? ` · ${t('acquire.season', { season: ask.season })}` : ''}
              </Text>
            ) : null}
          </Column>
          <Spacer />
          <Button
            size="sm"
            variant={auto ? 'primary' : 'glass'}
            icon={auto ? 'check' : undefined}
            label={t('acquire.autoMode')}
            onPress={() => setAuto((value) => !value)}
          />
        </Row>
      </Dialog.Header>
      <Dialog.Panel>
        <Column gap={16}>
          {auto ? (
            <Row gap={12} center wrap>
              <Text variant="body">{t('acquire.autoHint')}</Text>
              <Spacer />
              <Button
                variant="primary"
                label={t('acquire.autoRun')}
                loading={autoBusy}
                onPress={() => void runAuto()}
              />
            </Row>
          ) : null}
          {ask ? <AcquireList client={client} ask={ask} onTake={take} /> : null}
        </Column>
      </Dialog.Panel>
    </Dialog.Root>
  );
}