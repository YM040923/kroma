import { hasPermission } from '@kromatv/client/accounts';
import { useT } from '@kromatv/ui';
import { Badge, Button, Dialog, Row, Text } from '@kromatv/ui/kit';
import { useQueryClient } from '@tanstack/react-query';
import { type CSSProperties, useState } from 'react';
import { apiBase } from '#web/shared/lib/api';
import { useAuth } from '#web/shared/lib/auth';
import { useKromaClient } from '#web/shared/lib/web-runtime';

type ArtTab = 'poster' | 'backdrop';

interface ArtImage {
  url: string;
  width: number;
  height: number;
  language?: string | null;
  votes: number;
  /** A library's candidates name the title they came from; provider images
   *  carry no note and fall back to language/textless. */
  note?: string | null;
}

interface ArtView {
  current: { poster?: string | null; backdrop?: string | null; logo?: string | null };
  posters: ArtImage[];
  backdrops: ArtImage[];
}

const GRID: CSSProperties = {
  display: 'grid',
  gridTemplateColumns: 'repeat(auto-fill, minmax(160px, 1fr))',
  gap: 14,
  maxHeight: '52vh',
  overflow: 'auto',
  paddingRight: 4,
};

const CARD: CSSProperties = {
  padding: 0,
  border: 0,
  borderRadius: 'var(--radius-md)',
  overflow: 'hidden',
  background: 'var(--kroma-surface-2)',
  color: 'inherit',
  cursor: 'pointer',
  textAlign: 'left',
};

const IMG = (ratio: string): CSSProperties => ({
  width: '100%',
  aspectRatio: ratio,
  objectFit: 'cover',
  display: 'block',
});

/** The manual artwork picker on a title's detail page. The override is stored
 *  apart from the scraped metadata, so the next enrichment pass cannot undo it. */
export function ArtworkButton({ itemId, title }: Readonly<{ itemId: string; title: string }>) {
  const t = useT();
  const { user } = useAuth();
  const kroma = useKromaClient();
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [tab, setTab] = useState<ArtTab>('poster');
  const [view, setView] = useState<ArtView | null>(null);
  const [selected, setSelected] = useState<Partial<Record<ArtTab, string>>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  if (!user || !hasPermission(user, 'settings.manage')) {
    return null;
  }

  const call = async (method: 'GET' | 'PUT', body?: unknown) => {
    const response = await fetch(`${apiBase()}/api/items/${encodeURIComponent(itemId)}/artwork`, {
      method,
      headers: {
        Authorization: `Bearer ${kroma.sessionToken}`,
        ...(body === undefined ? {} : { 'Content-Type': 'application/json' }),
      },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    if (!response.ok) {
      throw new Error(`HTTP ${response.status}`);
    }
    return (await response.json()) as ArtView | { saved: boolean };
  };

  const load = async () => {
    setBusy(true);
    setError('');
    try {
      const next = (await call('GET')) as ArtView;
      setView(next);
      setSelected({});
      // A library's titles may carry backdrops only; opening on an empty
      // poster grid reads as "no artwork" when there is plenty one tab over.
      if (next.posters.length === 0 && next.backdrops.length > 0) {
        setTab('backdrop');
      }
    } catch {
      setError(t('artwork.loadFailed'));
    } finally {
      setBusy(false);
    }
  };

  const openDialog = () => {
    setOpen(true);
    setView(null);
    void load();
  };

  const save = async () => {
    if (!Object.keys(selected).length) return;
    setBusy(true);
    setError('');
    try {
      await call('PUT', selected);
      await queryClient.invalidateQueries();
      setOpen(false);
    } catch {
      setError(t('artwork.loadFailed'));
    } finally {
      setBusy(false);
    }
  };

  const reset = async () => {
    setBusy(true);
    setError('');
    try {
      await call('PUT', { [tab]: '' });
      await load();
    } catch {
      setError(t('artwork.loadFailed'));
    } finally {
      setBusy(false);
    }
  };

  const images = view ? (tab === 'poster' ? view.posters : view.backdrops) : [];

  return (
    <>
      <Button variant="glass" icon="photo" label={t('artwork.open')} size="sm" onPress={openDialog} />
      <Dialog.Root open={open} onClose={() => !busy && setOpen(false)} title={`${t('artwork.title')} · ${title}`} width="lg">
        <Dialog.Header>
          <Row gap={8} mt={6}>
            <Button
              size="sm"
              variant={tab === 'poster' ? 'primary' : 'ghost'}
              label={t('artwork.poster')}
              onPress={() => setTab('poster')}
            />
            <Button
              size="sm"
              variant={tab === 'backdrop' ? 'primary' : 'ghost'}
              label={t('artwork.backdrop')}
              onPress={() => setTab('backdrop')}
            />
          </Row>
        </Dialog.Header>
        <Dialog.Panel>
          {error ? (
            <Text variant="meta" color="danger" mb={10}>
              {error}
            </Text>
          ) : null}
          {!view ? (
            <Text variant="meta" color="textDim">
              …
            </Text>
          ) : images.length === 0 ? (
            <Text variant="meta" color="textDim">
              {t('artwork.empty')}
            </Text>
          ) : (
            <div style={GRID}>
              {images.map((image) => {
                const active =
                  selected[tab] === image.url ||
                  (!selected[tab] && view.current[tab] === image.url);
                return (
                  <button
                    key={image.url}
                    type="button"
                    style={{
                      ...CARD,
                      outline: active ? '2px solid var(--kroma-accent)' : 'none',
                    }}
                    onClick={() => setSelected((current) => ({ ...current, [tab]: image.url }))}
                  >
                    <img src={image.url} alt="" style={IMG(tab === 'poster' ? '2 / 3' : '16 / 9')} />
                    <Row between gap={8} px={8} py={6}>
                      <Text variant="meta" color="textDim" lines={1} minW={0}>
                        {image.note ?? (image.language || t('artwork.textless'))}
                      </Text>
                      {image.width > 0 ? (
                        <Text variant="meta" color="textDim">
                          {image.width}×{image.height}
                        </Text>
                      ) : null}
                    </Row>
                    {active ? (
                      <Badge tone="success">{t('artwork.current')}</Badge>
                    ) : null}
                  </button>
                );
              })}
            </div>
          )}
          <Row gap={12} mt={16}>
            <Button
              label={t('artwork.save')}
              variant="primary"
              size="sm"
              disabled={busy || !Object.keys(selected).length}
              onPress={() => void save()}
            />
            <Button
              label={t('artwork.reset')}
              variant="ghost"
              size="sm"
              disabled={busy || !view?.current[tab]}
              onPress={() => void reset()}
            />
            <Button
              label={t('common.cancel')}
              variant="ghost"
              size="sm"
              disabled={busy}
              onPress={() => setOpen(false)}
            />
          </Row>
        </Dialog.Panel>
      </Dialog.Root>
    </>
  );
}
