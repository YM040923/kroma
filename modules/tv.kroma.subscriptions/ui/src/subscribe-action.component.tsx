import type { SlotProps } from '@kromatv/module-sdk';
import { useT } from '@kromatv/module-sdk';
import { Badge, Box, Button, Dialog, Row, Text } from '@kromatv/ui/kit';
import { useState } from 'react';
import { type SubscriptionCandidate, useSubscribeApi } from './subscribe-api.hook';

type Props = SlotProps['title.detail.actions'];

/** The title page's subscribe action: the film is already identified here, so
 *  the dialog only asks WHERE to acquire it. The library is the first answer:
 *  a title it already plays is never sent to a source. */
export function SubscribeAction({ kind, tmdbId, title, owned }: Props) {
  const t = useT();
  const api = useSubscribeApi();
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [candidates, setCandidates] = useState<SubscriptionCandidate[] | null>(null);
  const [added, setAdded] = useState<ReadonlySet<string>>(new Set());

  const close = () => {
    if (busy) {
      return;
    }
    setOpen(false);
    setError('');
    setCandidates(null);
  };

  const search = async () => {
    if (tmdbId == null) {
      return;
    }
    setBusy(true);
    setError('');
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
    if (tmdbId == null) {
      return;
    }
    setBusy(true);
    setError('');
    try {
      await api.subscribe({
        kind,
        tmdbId,
        title,
        resource: candidate.fileName,
        price: candidate.price,
      });
      setAdded((current) => new Set(current).add(candidate.fileName));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  };

  if (tmdbId == null) {
    return null;
  }

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
          <Text variant="overline" color="textDim">
            {t('subscriptions.dialogTitle')}
          </Text>
          <Text variant="title" mt={4} lines={1}>
            {title}
          </Text>
        </Dialog.Header>
        <Dialog.Panel>
          {owned ? (
            <Text variant="body" color="success">
              {t('subscriptions.owned')}
            </Text>
          ) : (
            <>
              <Row between wrap gap={12} mb={16}>
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
                <Box gap={2}>
                  {candidates.map((candidate) => {
                    const isAdded = added.has(candidate.fileName);
                    return (
                      <Row
                        key={`${candidate.index}:${candidate.fileName}`}
                        between
                        gap={16}
                        py={12}
                        style={{
                          borderTopWidth: 1,
                          borderTopColor: 'var(--kroma-border)',
                          borderStyle: 'solid',
                        }}
                      >
                        <Box minW={0} shrink={1}>
                          <Text variant="label" lines={2}>
                            {candidate.fileName}
                          </Text>
                          <Row wrap gap={8} mt={4} align="center">
                            {candidate.size ? (
                              <Text variant="meta" color="textDim">
                                {candidate.size}
                              </Text>
                            ) : null}
                            {candidate.tags.map((tag) => (
                              <Badge key={tag} tone="info">
                                {tag}
                              </Badge>
                            ))}
                            <Text variant="meta" color="accentText">
                              {candidate.price > 0
                                ? t('subscriptions.points', { points: candidate.price })
                                : t('subscriptions.free')}
                            </Text>
                          </Row>
                        </Box>
                        <Button
                          label={isAdded ? t('subscriptions.added') : t('subscriptions.add')}
                          size="sm"
                          variant={isAdded ? 'ghost' : 'primary'}
                          disabled={busy || isAdded}
                          onPress={() => void subscribe(candidate)}
                        />
                      </Row>
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
