// The acquisition panel: every "get this" entry opens this, one place where the
// releases a search found are listed, picked, and confirmed. The tabs name the
// sources the server can ask; MADREEL is the one wired today, so the other two
// say why they are empty instead of pretending to search.

import type { KromaClient } from '@kromatv/client';
import { apiErrorText } from '@kromatv/client';
import type { AcquisitionCandidate, AcquisitionCandidates, Cloud115Candidate } from '@kromatv/client/acquisition';
import type { Translate } from '@kromatv/i18n';
import { useT } from '@kromatv/ui';
import {
  Badge,
  Box,
  Button,
  Column,
  Dialog,
  EmptyState,
  Row,
  SegmentGroup,
  Skeleton,
  Spacer,
  Text,
} from '@kromatv/ui/kit';
import { useCallback, useEffect, useState } from 'react';
import type { AcquisitionTarget } from '#web/shared/lib/use-acquisition.hook';

/** The tabs the panel can show. `madreel` is the only source with a provider
 *  behind it today; the rest are named so the shape of the thing is honest
 *  rather than hidden. */
export type AcquireTab = 'madreel' | 'bt' | 'site';

const TABS: readonly AcquireTab[] = ['madreel', 'bt', 'site'];

const TAB_LABEL = {
  madreel: 'acquire.tabMadreel',
  bt: 'acquire.tabBt',
  site: 'acquire.tabSite',
} as const;

/** What the panel needs to ask for one title. */
export interface AcquirePanelTarget {
  target: AcquisitionTarget;
  /** The library the title aims at, when the page knows it. */
  library?: string | null;
}

interface PanelProps {
  open: boolean;
  onClose: () => void;
  client: KromaClient;
  ask: AcquirePanelTarget | null;
  /** What the caller does with the release it picked, or with the server's own
   *  choice (`null`) when the panel runs in automatic mode. */
  onTake: (candidate: Cloud115Candidate | null) => Promise<void>;
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
  const price = one.candidate.price;
  if (price == null) {
    return t('acquire.priceUnknown');
  }
  if (price === 0) {
    return t('acquire.priceFree');
  }
  return t('acquire.pricePoints', { points: price });
}

function fileNameOf(candidate: Cloud115Candidate): string {
  return candidate.fileName || candidate.resource || candidate.shareCode || `#${candidate.index}`;
}

function CandidateRow({
  one,
  busy,
  onPick,
}: Readonly<{ one: AcquisitionCandidate; busy: boolean; onPick: () => void }>) {
  const t = useT();
  const size = bytesOf(one.candidate.sizeBytes ?? null) || one.candidate.size || '';
  return (
    <Row gap={12} center>
      <Column flex gap={4}>
        <Row gap={6} center wrap>
          {one.recommended ? <Badge tone="info">{t('acquire.recommended')}</Badge> : null}
          {one.needsConfirm ? <Badge tone="warning">{t('acquire.needsConfirm')}</Badge> : null}
          <Text variant="body" lines={2}>
            {fileNameOf(one.candidate)}
          </Text>
        </Row>
        <Row gap={10} wrap>
          {size ? (
            <Text variant="meta" color="textMuted">
              {size}
            </Text>
          ) : null}
          <Text variant="meta" color="textMuted">
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

/** The panel body for one target: fetch the list once per open, and let the
 *  caller take a row. */
function AcquireList({
  client,
  ask,
  tab,
  onTake,
}: Readonly<{
  client: KromaClient;
  ask: AcquirePanelTarget;
  tab: AcquireTab;
  onTake: (candidate: Cloud115Candidate) => Promise<void>;
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
  }, [ask.target.kind, ask.target.tmdbId, ask.target.title, ask.target.year, ask.library, client, t]);

  const take = useCallback(
    async (candidate: Cloud115Candidate) => {
      setTaking(true);
      try {
        await onTake(candidate);
      } finally {
        setTaking(false);
      }
    },
    [onTake],
  );

  if (tab !== 'madreel') {
    return (
      <EmptyState.Root>
        <EmptyState.Title>{t(TAB_LABEL[tab])}</EmptyState.Title>
        <EmptyState.Hint>{t('acquire.tabNotWired')}</EmptyState.Hint>
      </EmptyState.Root>
    );
  }

  if (loading) {
    return (
      <Column gap={8}>
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
          key={one.candidate.index}
          one={one}
          busy={taking}
          onPick={() => void take(one.candidate)}
        />
      ))}
    </Column>
  );
}

/** The one acquisition panel. Closing it never cancels work the server already
 *  started: the attempt keeps its own id and the task list follows it. */
export function AcquirePanel({ open, onClose, client, ask, onTake }: Readonly<PanelProps>) {
  const t = useT();
  const [tab, setTab] = useState<AcquireTab>('madreel');
  const [auto, setAuto] = useState(false);
  const [autoBusy, setAutoBusy] = useState(false);

  const take = useCallback(
    async (candidate: Cloud115Candidate) => {
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
          <Text variant="title">{t('acquire.title')}</Text>
          <Spacer />
          <Button
            size="sm"
            variant={auto ? 'primary' : 'glass'}
            icon={auto ? 'check' : undefined}
            label={t('acquire.autoMode')}
            onPress={() => setAuto((value) => !value)}
          />
        </Row>
        {ask ? (
          <Text variant="meta" color="textMuted">
            {ask.target.title}
            {ask.target.year ? ` (${ask.target.year})` : ''}
          </Text>
        ) : null}
      </Dialog.Header>
      <Dialog.Panel>
        <Column gap={16}>
          <SegmentGroup.Root
            value={tab}
            onValueChange={(next) => setTab(next as AcquireTab)}
            label={t('acquire.source')}
          >
            {TABS.map((one) => (
              <SegmentGroup.Item key={one} value={one}>
                <SegmentGroup.Label>{t(TAB_LABEL[one])}</SegmentGroup.Label>
              </SegmentGroup.Item>
            ))}
          </SegmentGroup.Root>
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
          {ask ? <AcquireList client={client} ask={ask} tab={tab} onTake={take} /> : <Box />}
        </Column>
      </Dialog.Panel>
    </Dialog.Root>
  );
}
