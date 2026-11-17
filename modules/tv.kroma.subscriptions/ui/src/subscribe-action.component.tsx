import type { SlotProps } from '@kromatv/module-sdk';
import { useT } from '@kromatv/module-sdk';
import { Badge, Box, Button, Dialog, Row, Text } from '@kromatv/ui/kit';
import { useState } from 'react';
import { type SubscriptionCandidate, useSubscribeApi } from './subscribe-api.hook';

type Props = SlotProps['title.detail.actions'];

const POSTER: React.CSSProperties = {
  width: 76,
  height: 114,
  borderRadius: 'var(--radius-md)',
  objectFit: 'cover',
  background: 'var(--kroma-surface-2)',
};

function sizeOf(candidate: SubscriptionCandidate): string {
  if (candidate.size) return candidate.size;
  if (candidate.sizeBytes <= 0) return '';
  const gb = candidate.sizeBytes / 1024 ** 3;
  return `${gb >= 10 ? gb.toFixed(1) : gb.toFixed(2)} GB`;
}

/** The title page's subscribe action: the film is already identified here, so
 *  the dialog only asks WHERE to acquire it. The library is the first answer:
 *  a title it already plays is never sent to a source. */
export function SubscribeAction({ kind, tmdbId, title, year, poster, owned }: Props) {
  const t = useT();
  const api = useSubscribeApi();
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [candidates, setCandidates] = useState<SubscriptionCandidate[] | null>(null);
  const [added, setAdded] = useState<ReadonlySet<string>>(new Set());
  const [confirming, setConfirming] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);

  const close = () => {
    if (busy) return;
    setOpen(false);
    setError('');
    setCandidates(null);
    setConfirming(null);
  };

  const search = async () => {
    if (tmdbId == null) return;
    setBusy(true);
    setError('');
    setCandidates(null);
    try {
      const view = await api.search({ kind, tmdbId, title });
      setCandidates(view.candidates);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  };

  const subscribe = async (candidate: SubscriptionCandidate) => {
    if (tmdbId == null) return;
    const key = `${candidate.index}:${candidate.fileName}`;
    if (confirming !== key) {
      setConfirming(key);
      return;
    }
    setBusy(true);
    setError('');
    try {
      await api.subscribe({
        kind,
        tmdbId,
        title,
        resource: candidate.fileName || candidate.shareCode || `#${candidate.index}`,
        index: candidate.index,
        price: candidate.price ?? 0,
        poster,
        year: year ?? 0,
        link: candidate.link,
      });
      setAdded((current) => new Set(current).add(key));
      setConfirming(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  };

  const copy = async (candidate: SubscriptionCandidate) => {
    const link = candidate.link || candidate.shareCode;
    if (!link) return;
    try {
      await navigator.clipboard.writeText(link);
      setCopied(`${candidate.index}`);
      setTimeout(() => setCopied(null), 1500);
    } catch {
      setError(link);
    }
  };

  if (tmdbId == null) return null;

  return (
    <>
      <Button
        variant="glass"
        icon="repeat"
        label={t('subscriptions.action')}
        onPress={() => setOpen(true)}
      />
      <Dialog.Root open={open} onClose={close} title={title} width="lg">
        <Dialog.Header>
          <Row gap={14} align="flex-start">
            {poster ? <img src={poster} alt="" style={POSTER} /> : null}
            <Box minW={0}>
              <Text variant="overline" color="textDim">
                {t('subscriptions.dialogTitle')}
              </Text>
              <Text variant="title" mt={4} lines={2}>
                {title}
              </Text>
              <Row gap={8} mt={6}>
                <Badge tone="neutral">
                  {t(kind === 'show' ? 'subscriptions.shows' : 'subscriptions.movies')}
                </Badge>
                {year ? <Text variant="meta" color="textDim">{year}</Text> : null}
              </Row>
            </Box>
          </Row>
        </Dialog.Header>
        <Dialog.Panel>
          {owned ? (
            <Text variant="body" color="success">
              {t('subscriptions.owned')}
            </Text>
          ) : (
            <>
              <Row between wrap gap={12} mb={14}>
                <Box>
                  <Text variant="cardTitle">{t('subscriptions.source')}</Text>
                  <Text variant="meta" color="textDim" mt={2}>
                    {t('subscriptions.sourceHint')}
                  </Text>
                </Box>
                <Button
                  label={busy ? t('subscriptions.searching') : t('subscriptions.search')}
                  icon="search"
                  variant="primary"
                  size="sm"
                  disabled={busy}
                  onPress={() => void search()}
                />
              </Row>
              {error ? (
                <Text variant="meta" color="danger" mb={12}>
                  {error}
                </Text>
              ) : null}
              {candidates == null ? null : candidates.length === 0 ? (
                <Text variant="meta" color="textDim">
                  {t('subscriptions.noResults')}
                </Text>
              ) : (
                <Box gap={10}>
                  {candidates.map((candidate) => {
                    const key = `${candidate.index}:${candidate.fileName}`;
                    const isAdded = added.has(key);
                    const needsConfirm = confirming === key;
                    return (
                      <Box
                        key={key}
                        border="border"
                        radius="lg"
                        px={14}
                        py={12}
                        bg="wash"
                      >
                        <Row between gap={14} align="flex-start">
                          <Box minW={0} shrink={1}>
                            <Text variant="label" lines={2}>
                              {candidate.fileName || `#${candidate.index} ${candidate.shareCode}`}
                            </Text>
                            <Row wrap gap={8} mt={6} align="center">
                              {sizeOf(candidate) ? (
                                <Text variant="meta" color="textDim">{sizeOf(candidate)}</Text>
                              ) : null}
                              {candidate.tags.map((tag) => (
                                <Badge key={tag} tone="info">
                                  {tag}
                                </Badge>
                              ))}
                              {candidate.alreadyUnlocked ? (
                                <Badge tone="success">{t('subscriptions.unlocked')}</Badge>
                              ) : null}
                            </Row>
                            <Row wrap gap={12} mt={6}>
                              <Text variant="meta" color={candidate.price != null ? 'accentText' : 'textDim'}>
                                {candidate.price != null
                                  ? t('subscriptions.points', { points: candidate.price })
                                  : t('subscriptions.priceUnknown')}
                              </Text>
                              {candidate.unlockCount > 0 ? (
                                <Text variant="meta" color="textDim">
                                  {t('subscriptions.unlockCount', { count: candidate.unlockCount })}
                                </Text>
                              ) : null}
                              {candidate.sharer ? (
                                <Text variant="meta" color="textDim">
                                  {t('subscriptions.sharer', { name: candidate.sharer })}
                                </Text>
                              ) : null}
                              {candidate.date ? (
                                <Text variant="meta" color="textDim">{candidate.date}</Text>
                              ) : null}
                            </Row>
                            {candidate.probeError ? (
                              <Text variant="meta" color="warning" mt={4}>
                                {candidate.probeError}
                              </Text>
                            ) : null}
                          </Box>
                          <Row gap={8} shrink={0}>
                            <Button
                              label={copied === `${candidate.index}` ? t('subscriptions.copied') : t('subscriptions.copy')}
                              size="sm"
                              variant="ghost"
                              disabled={!candidate.link && !candidate.shareCode}
                              onPress={() => void copy(candidate)}
                            />
                            <Button
                              label={
                                isAdded
                                  ? t('subscriptions.added')
                                  : needsConfirm
                                    ? candidate.price != null
                                      ? t('subscriptions.confirmPoints', { points: candidate.price })
                                      : t('subscriptions.confirmSubscribe')
                                    : t('subscriptions.add')
                              }
                              size="sm"
                              variant={isAdded ? 'ghost' : 'primary'}
                              disabled={busy || isAdded}
                              onPress={() => void subscribe(candidate)}
                            />
                          </Row>
                        </Row>
                      </Box>
                    );
                  })}
                </Box>
              )}
            </>
          )}
        </Dialog.Panel>
      </Dialog.Root>
    </>
  );
}
