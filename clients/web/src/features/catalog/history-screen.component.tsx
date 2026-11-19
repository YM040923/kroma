// The personal watch history: what is half-watched, what is finished, and the
// edits for both. Rows rather than a poster grid because every row owns
// actions (resume, forget, clear the whole show), which a poster cannot carry.

import type { ContinueItem, ItemId, ShowId } from '@kromatv/client/media';
import type { WatchedEntry } from '@kromatv/client/playback';
import { episodeTag } from '@kromatv/core';
import { useLocale, useT } from '@kromatv/ui';
import {
  Box,
  Button,
  Checkbox,
  EmptyState,
  IconButton,
  PageHeader,
  Progress,
  Row,
  Section,
  Surface,
  Text,
  useConfirm,
} from '@kromatv/ui/kit';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { type ReactNode, useState } from 'react';
import { useAuth } from '#web/shared/lib/auth';
import { useWebRuntime } from '#web/shared/lib/web-runtime';
import { Image, PageFrame, SkeletonRow } from '#web/shared/ui';
import { RouteLink } from '#web/shared/ui/route-link.component';

const THUMB_W = 148;

const sDefinition = {
  thumb: { width: THUMB_W, aspectRatio: 16 / 9, radius: 'md', shrink: 0, overflow: 'hidden' },
} as const;

function progressOf(row: ContinueItem): number {
  const duration = row.durationMs ?? row.item.durationMs ?? 0;
  if (duration <= 0) {
    return 0;
  }
  return Math.min(100, Math.round((row.positionMs / duration) * 100));
}

function clock(ms: number): string {
  const total = Math.max(0, Math.round(ms / 1000));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  const mm = hours > 0 ? String(minutes).padStart(2, '0') : String(minutes);
  return hours > 0
    ? `${hours}:${mm}:${String(seconds).padStart(2, '0')}`
    : `${mm}:${String(seconds).padStart(2, '0')}`;
}

function DetailLink({
  item,
  children,
}: Readonly<{ item: ContinueItem['item']; children?: ReactNode }>) {
  return item.showId ? (
    <RouteLink to="/shows/$id" params={{ id: item.showId }}>
      {children}
    </RouteLink>
  ) : (
    <RouteLink to="/movies/$id" params={{ id: item.id }}>
      {children}
    </RouteLink>
  );
}

function faceOf(item: ContinueItem['item'], filmLabel: string): { overline: string; title: string } {
  if (item.kind !== 'episode') {
    return { overline: filmLabel, title: item.title.trim() };
  }
  return {
    overline: [item.showTitle?.trim(), episodeTag(item)].filter(Boolean).join(' · '),
    title: (item.episodeTitle ?? item.title).trim(),
  };
}

function Art({ row }: Readonly<{ row: ContinueItem | WatchedEntry }>) {
  const { client } = useAuth();
  const item = row.item;
  const art =
    client.media.artwork.backdropFor(item, THUMB_W * 2) ??
    client.media.artwork.posterFor(item, THUMB_W * 2);
  return <Image src={art ?? undefined} style={sDefinition.thumb} />;
}

function RowBody({
  row,
  overline,
  title,
  children,
}: Readonly<{
  row: ContinueItem | WatchedEntry;
  overline: string;
  title: string;
  children?: ReactNode;
}>) {
  return (
    <>
      <DetailLink item={row.item}>
        <Art row={row} />
      </DetailLink>
      <Box gap={4} grow={1} shrink={1} minW={0}>
        {overline ? (
          <Text variant="meta" color="accentText" numberOfLines={1}>
            {overline}
          </Text>
        ) : null}
        <DetailLink item={row.item}>
          <Text variant="title" numberOfLines={2}>
            {title}
          </Text>
        </DetailLink>
        {children}
      </Box>
    </>
  );
}

function ProgressRow({
  row,
  selected,
  selectMode,
  onToggle,
  onForget,
  onClearShow,
  busy,
}: Readonly<{
  row: ContinueItem;
  selected: boolean;
  selectMode: boolean;
  onToggle: () => void;
  onForget: () => void;
  onClearShow: () => void;
  busy: boolean;
}>) {
  const t = useT();
  const face = faceOf(row.item, t('content.film'));
  const percent = progressOf(row);
  const duration = row.durationMs ?? row.item.durationMs ?? 0;
  return (
    <Surface tone="outline" pad="sm">
      <Row gap={14} align="center">
        {selectMode ? (
          <Checkbox
            checked={selected}
            onCheckedChange={onToggle}
            label={t('history.actions', { title: face.title })}
          />
        ) : null}
        <RowBody row={row} overline={face.overline} title={face.title}>
          <Box mt={6} gap={4} w="100%">
            <Progress value={percent / 100} label={t('history.progressLabel', { percent })} />
            <Row between gap={8}>
              <Text variant="meta" color="textDim">
                {t('history.progressLabel', { percent })}
              </Text>
              {duration > 0 ? (
                <Text variant="meta" color="textDim">
                  {clock(row.positionMs)} / {clock(duration)}
                </Text>
              ) : null}
            </Row>
          </Box>
        </RowBody>
        <Row gap={8} align="center" shrink={0}>
          <Button size="sm" label={t('history.resume')} asChild>
            <RouteLink to="/watch/$id" params={{ id: row.item.id }} />
          </Button>
          {row.item.showId ? (
            <IconButton
              control="sm"
              variant="ghost"
              icon="x"
              label={t('history.clearShow')}
              disabled={busy}
              onPress={onClearShow}
            />
          ) : null}
          <IconButton
            control="sm"
            variant="ghost"
            icon="trash"
            label={t('history.remove')}
            disabled={busy}
            onPress={onForget}
          />
        </Row>
      </Row>
    </Surface>
  );
}

function WatchedRow({
  entry,
  selected,
  selectMode,
  onToggle,
  onUnwatch,
  busy,
}: Readonly<{
  entry: WatchedEntry;
  selected: boolean;
  selectMode: boolean;
  onToggle: () => void;
  onUnwatch: () => void;
  busy: boolean;
}>) {
  const t = useT();
  const locale = useLocale();
  const face = faceOf(entry.item, t('content.film'));
  return (
    <Surface tone="outline" pad="sm">
      <Row gap={14} align="center">
        {selectMode ? (
          <Checkbox
            checked={selected}
            onCheckedChange={onToggle}
            label={t('history.actions', { title: face.title })}
          />
        ) : null}
        <RowBody row={entry} overline={face.overline} title={face.title}>
          <Text variant="meta" color="textDim" mt={4}>
            {t('history.watchedOn', {
              date: new Date(entry.watchedAt).toLocaleDateString(locale, {
                year: 'numeric',
                month: 'short',
                day: 'numeric',
              }),
            })}
          </Text>
        </RowBody>
        <Button
          size="sm"
          variant="ghost"
          label={t('history.unwatch')}
          disabled={busy}
          onPress={onUnwatch}
        />
      </Row>
    </Surface>
  );
}

export function HistoryScreen() {
  const t = useT();
  const { client, ready, user } = useAuth();
  const webRuntime = useWebRuntime();
  const queryClient = useQueryClient();
  const confirm = useConfirm();
  const historyQuery = webRuntime.queries.userQueries.history();
  const { data, isPending } = useQuery({ ...historyQuery, enabled: ready && !!user });
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set());
  const [selectMode, setSelectMode] = useState(false);
  const [busy, setBusy] = useState(false);

  if (!ready || !user) {
    return null;
  }

  const refresh = () => queryClient.invalidateQueries({ queryKey: historyQuery.queryKey });

  const run = async (work: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await work();
      await refresh();
    } finally {
      setBusy(false);
    }
  };

  const toggle = (id: string, next: boolean) => {
    setSelected((current) => {
      const copy = new Set(current);
      if (next) {
        copy.add(id);
      } else {
        copy.delete(id);
      }
      return copy;
    });
  };

  const removeSelected = async () => {
    const ids = [...selected] as ItemId[];
    if (ids.length === 0) {
      return;
    }
    const ok = await confirm({
      title: t('history.removeTitle'),
      message: t('history.removeMessage', { title: t('history.selectedCount', { count: ids.length }) }),
      confirmLabel: t('history.removeSelected', { count: ids.length }),
      cancelLabel: t('common.cancel'),
      destructive: true,
    });
    if (!ok) {
      return;
    }
    await run(async () => {
      await client.playback.clearHistoryItems(ids);
      setSelected(new Set());
      setSelectMode(false);
    });
  };

  const clearAll = async () => {
    const ok = await confirm({
      title: t('history.clearAllTitle'),
      message: t('history.clearAllMessage'),
      confirmLabel: t('history.clearAll'),
      cancelLabel: t('common.cancel'),
      destructive: true,
    });
    if (ok) {
      await run(() => client.playback.clearHistory());
    }
  };

  const forget = (id: ItemId, title: string) => {
    void (async () => {
      const ok = await confirm({
        title: t('history.removeTitle'),
        message: t('history.removeMessage', { title }),
        confirmLabel: t('history.remove'),
        cancelLabel: t('common.cancel'),
        destructive: true,
      });
      if (ok) {
        await run(() => client.playback.forget(id));
      }
    })();
  };

  const unwatch = (id: ItemId) => run(() => client.playback.unmarkWatched(id));

  const clearShow = (showId: ShowId, title: string) => {
    void (async () => {
      const ok = await confirm({
        title: t('history.clearShowTitle'),
        message: t('history.clearShowMessage', { title }),
        confirmLabel: t('history.clearShow'),
        cancelLabel: t('common.cancel'),
        destructive: true,
      });
      if (ok) {
        await run(() => client.playback.clearShowHistory(showId));
      }
    })();
  };

  const watched = data?.watched ?? [];
  const inProgress = data?.inProgress ?? [];
  const total = watched.length + inProgress.length;

  return (
    <PageFrame>
      <PageHeader.Root>
        <PageHeader.Title suffix={total > 0 ? String(total) : undefined}>
          {t('history.title')}
        </PageHeader.Title>
        <PageHeader.Subtitle>{t('history.subtitle')}</PageHeader.Subtitle>
        <PageHeader.Actions>
          {selectMode ? (
            <>
              <Button
                size="sm"
                variant="ghost"
                label={t('history.cancelSelect')}
                onPress={() => {
                  setSelectMode(false);
                  setSelected(new Set());
                }}
              />
              <Button
                size="sm"
                variant="danger"
                label={t('history.removeSelected', { count: selected.size })}
                disabled={selected.size === 0 || busy}
                onPress={() => void removeSelected()}
              />
            </>
          ) : (
            <>
              <Button
                size="sm"
                variant="ghost"
                label={t('history.select')}
                disabled={total === 0}
                onPress={() => setSelectMode(true)}
              />
              <Button
                size="sm"
                variant="dangerGhost"
                label={t('history.clearAll')}
                disabled={total === 0 || busy}
                onPress={() => void clearAll()}
              />
            </>
          )}
        </PageHeader.Actions>
      </PageHeader.Root>

      {isPending ? (
        <Box mt={24}>
          <SkeletonRow count={6} />
        </Box>
      ) : total === 0 ? (
        <Box mt={32}>
          <EmptyState.Root icon="history">
            <EmptyState.Title>{t('history.empty')}</EmptyState.Title>
            <EmptyState.Hint>{t('history.emptyHint')}</EmptyState.Hint>
            <EmptyState.Actions>
              <Button size="sm" label={t('nav.films')} asChild>
                <RouteLink to="/movies" />
              </Button>
            </EmptyState.Actions>
          </EmptyState.Root>
        </Box>
      ) : (
        <Box gap={36} mt={28}>
          <Section.Root>
            <Section.Header>
              <Section.Title>{t('history.inProgress')}</Section.Title>
            </Section.Header>
            {inProgress.length === 0 ? (
              <Text variant="body" color="textDim">
                {t('history.emptyProgress')}
              </Text>
            ) : (
              <Box gap={10}>
                {inProgress.map((row) => (
                  <ProgressRow
                    key={row.item.id}
                    row={row}
                    selectMode={selectMode}
                    selected={selected.has(row.item.id)}
                    onToggle={() => toggle(row.item.id, !selected.has(row.item.id))}
                    onForget={() => forget(row.item.id, row.item.title)}
                    onClearShow={() =>
                      row.item.showId ? clearShow(row.item.showId, row.item.showTitle ?? row.item.title) : undefined
                    }
                    busy={busy}
                  />
                ))}
              </Box>
            )}
          </Section.Root>

          <Section.Root>
            <Section.Header>
              <Section.Title>{t('history.watched')}</Section.Title>
            </Section.Header>
            {watched.length === 0 ? (
              <Text variant="body" color="textDim">
                {t('history.emptyWatched')}
              </Text>
            ) : (
              <Box gap={10}>
                {watched.map((entry) => (
                  <WatchedRow
                    key={entry.item.id}
                    entry={entry}
                    selectMode={selectMode}
                    selected={selected.has(entry.item.id)}
                    onToggle={() => toggle(entry.item.id, !selected.has(entry.item.id))}
                    onUnwatch={() => void unwatch(entry.item.id)}
                    busy={busy}
                  />
                ))}
              </Box>
            )}
          </Section.Root>
        </Box>
      )}
    </PageFrame>
  );
}
