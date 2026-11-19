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
  /** Libraries only: their generated/uploaded cover is what 小幻 draws. */
  library?: boolean;
  name?: string;
  posters: ArtImage[];
  backdrops: ArtImage[];
}

const COVER_W = 1280;
const COVER_H = 720;

function loadImage(url: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.crossOrigin = 'anonymous';
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error('image'));
    image.src = url;
  });
}

function drawCoverImage(
  ctx: CanvasRenderingContext2D,
  image: HTMLImageElement,
  x: number,
  y: number,
  width: number,
  height: number,
) {
  const scale = Math.max(width / image.naturalWidth, height / image.naturalHeight);
  const drawWidth = image.naturalWidth * scale;
  const drawHeight = image.naturalHeight * scale;
  ctx.drawImage(
    image,
    x + (width - drawWidth) / 2,
    y + (height - drawHeight) / 2,
    drawWidth,
    drawHeight,
  );
}

/** A 16:9 cover: four poster columns, or one image, with the library name
 *  burned in because Jellyfin clients draw the cover as the whole card. */
async function composeCover(
  images: string[],
  name: string,
  layout: 'collage' | 'single',
): Promise<Blob | null> {
  if (!images.length) return null;
  const canvas = document.createElement('canvas');
  canvas.width = COVER_W;
  canvas.height = COVER_H;
  const ctx = canvas.getContext('2d');
  if (!ctx) return null;
  ctx.fillStyle = '#111';
  ctx.fillRect(0, 0, COVER_W, COVER_H);
  const sources = await Promise.all(images.slice(0, 4).map(loadImage));
  if (!sources[0]) return null;
  if (layout === 'single') {
    drawCoverImage(ctx, sources[0], 0, 0, COVER_W, COVER_H);
  } else {
    const width = COVER_W / Math.max(sources.length, 1);
    sources.forEach((image, index) => {
      drawCoverImage(ctx, image, index * width, 0, width, COVER_H);
    });
  }
  const wash = ctx.createLinearGradient(0, COVER_H * 0.38, 0, COVER_H);
  wash.addColorStop(0, 'rgba(0,0,0,0)');
  wash.addColorStop(1, 'rgba(0,0,0,0.82)');
  ctx.fillStyle = wash;
  ctx.fillRect(0, 0, COVER_W, COVER_H);
  ctx.font = '700 76px system-ui, "Segoe UI", "Microsoft YaHei", sans-serif';
  ctx.fillStyle = '#fff';
  ctx.textBaseline = 'alphabetic';
  ctx.shadowColor = 'rgba(0,0,0,0.55)';
  ctx.shadowBlur = 18;
  let title = name.trim() || 'KROMA';
  while (ctx.measureText(title).width > COVER_W - 120 && title.length > 1) {
    title = title.slice(0, -1);
  }
  ctx.fillText(title, 60, COVER_H - 64);
  return await new Promise((resolve) => canvas.toBlob((blob) => resolve(blob), 'image/png'));
}

const GRID: CSSProperties = {
  display: 'grid',
  gridTemplateColumns: 'repeat(auto-fill, minmax(160px, 1fr))',
  // Auto rows otherwise resolve to a sliver and the panel's stretch squeezes
  // every card to it, cropping the image to a strip.
  gridAutoRows: 'max-content',
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
  const [mode, setMode] = useState<'pick' | 'generate' | 'upload'>('pick');
  const [picked, setPicked] = useState<string[]>([]);
  const [hasTitle, setHasTitle] = useState(false);
  const [saved, setSaved] = useState(false);

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
      setPicked(next.library ? next.posters.slice(0, 4).map((image) => image.url) : []);
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
    setMode('pick');
    setPicked([]);
    setSaved(false);
    void load();
  };

  const uploadCover = async (blob: Blob) => {
    const response = await fetch(
      `${apiBase()}/api/items/${encodeURIComponent(itemId)}/artwork/cover`,
      {
        method: 'POST',
        headers: {
          Authorization: `Bearer ${kroma.sessionToken}`,
          'Content-Type': blob.type || 'image/png',
        },
        body: blob,
      },
    );
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    setSaved(true);
    await load();
    setMode('pick');
  };

  const generate = async () => {
    if (!view || picked.length === 0) return;
    setBusy(true);
    setError('');
    try {
      const blob = await composeCover(picked, view.name ?? '', 'collage');
      if (!blob) throw new Error('empty');
      await uploadCover(blob);
    } catch {
      setError(t('artwork.loadFailed'));
    } finally {
      setBusy(false);
    }
  };

  const uploadFile = async (file: File | null) => {
    if (!file || !view) return;
    setBusy(true);
    setError('');
    try {
      const url = URL.createObjectURL(file);
      const blob = hasTitle
        ? file
        : await composeCover([url], view.name ?? '', 'single');
      URL.revokeObjectURL(url);
      if (!blob) throw new Error('empty');
      await uploadCover(blob);
    } catch {
      setError(t('artwork.loadFailed'));
    } finally {
      setBusy(false);
    }
  };

  const togglePicked = (url: string) => {
    setPicked((current) => current.includes(url)
      ? current.filter((entry) => entry !== url)
      : current.length >= 4 ? current : [...current, url]);
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
          {saved ? (
            <Text variant="meta" color="success" mb={10}>
              {t('artwork.coverSaved')}
            </Text>
          ) : null}
          {view?.library ? (
            <Row gap={8} mb={12}>
              <Button
                size="sm"
                variant="ghost"
                label={t('artwork.generate')}
                onPress={() => setMode(mode === 'generate' ? 'pick' : 'generate')}
              />
              <Button
                size="sm"
                variant="ghost"
                label={t('artwork.upload')}
                onPress={() => setMode(mode === 'upload' ? 'pick' : 'upload')}
              />
            </Row>
          ) : null}
          {view?.library && mode === 'generate' ? (
            <>
              <Text variant="meta" color="textDim" mb={8}>
                {t('artwork.generateHint')}
              </Text>
              {view.posters.length === 0 ? (
                <Text variant="meta" color="textDim">
                  {t('artwork.empty')}
                </Text>
              ) : (
                <div style={GRID}>
                  {view.posters.map((image) => {
                    const active = picked.includes(image.url);
                    return (
                      <button
                        key={image.url}
                        type="button"
                        style={{
                          ...CARD,
                          outline: active ? '2px solid var(--kroma-accent)' : 'none',
                        }}
                        onClick={() => togglePicked(image.url)}
                      >
                        <img src={image.url} alt="" style={IMG('2 / 3')} />
                        {active ? <Badge tone="success">{picked.indexOf(image.url) + 1}</Badge> : null}
                      </button>
                    );
                  })}
                </div>
              )}
              <Row gap={12} mt={16}>
                <Button
                  label={t('artwork.generate')}
                  variant="primary"
                  size="sm"
                  disabled={busy || picked.length === 0}
                  onPress={() => void generate()}
                />
                <Button
                  label={t('common.cancel')}
                  variant="ghost"
                  size="sm"
                  disabled={busy}
                  onPress={() => setMode('pick')}
                />
              </Row>
            </>
          ) : null}
          {view?.library && mode === 'upload' ? (
            <>
              <Text variant="meta" color="textDim" mb={8}>
                {t('artwork.uploadHint')}
              </Text>
              <input
                type="file"
                accept="image/*"
                disabled={busy}
                onChange={(event) => void uploadFile(event.target.files?.[0] ?? null)}
              />
              <Row gap={8} mt={10}>
                <input
                  id="library-cover-has-title"
                  type="checkbox"
                  checked={hasTitle}
                  onChange={(event) => setHasTitle(event.target.checked)}
                />
                <Text variant="meta" color="textDim">
                  {t('artwork.hasTitle')}
                </Text>
              </Row>
              <Row gap={12} mt={16}>
                <Button
                  label={t('common.cancel')}
                  variant="ghost"
                  size="sm"
                  disabled={busy}
                  onPress={() => setMode('pick')}
                />
              </Row>
            </>
          ) : null}
          {mode !== 'pick' ? null : (
          <>
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
          </>
          )}
        </Dialog.Panel>
      </Dialog.Root>
    </>
  );
}
