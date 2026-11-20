import type { Activity } from '@kromatv/client/admin';
import type { AdminLibrary } from '@kromatv/client/library';
import { langName } from '@kromatv/core';
import { useFormat, useT } from '@kromatv/ui';
import {
  Box,
  Button,
  DataField,
  Divider,
  EmptyState,
  Grid,
  Icon,
  type IconName,
  Progress,
  Row,
  Surface,
  Text,
} from '@kromatv/ui/kit';
import { useNavigate } from '@tanstack/react-router';
import { useState } from 'react';
import {
  AddLibraryModal,
  normalizeLibKind,
} from '#web/features/admin/libraries-modals';
import { Pill } from '#web/features/admin/pill.component';
import { Denied, PageHeader, useCap, usePoll } from '#web/features/admin/shell';
import { imageUrl } from '#web/shared/lib/api';
import { useAuth } from '#web/shared/lib/auth';
import { TableSkeleton } from '#web/shared/ui';

const ICONS: Record<string, IconName> = {
  film: 'movie',
  tv: 'device-tv',
  music: 'music',
  photo: 'photo',
};

const KIND_LABEL = {
  '': 'admin.typeAuto',
  movies: 'admin.typeMovies',
  shows: 'admin.typeShows',
  mixed: 'admin.typeMixed',
  photos: 'admin.typePhotos',
  other: 'admin.typeOther',
} as const;

export function LibrariesScreen() {
  if (!useCap('library.manage')) {
    return <Denied />;
  }
  return <LibrariesPageInner />;
}

function LibrariesPageInner() {
  const t = useT();
  const { client } = useAuth();
  const navigate = useNavigate();
  const canScrape = useCap('settings.manage');
  const { data, reload } = usePoll(['admin', 'libraries'], () => client.library.list(), 8000);
  const { data: activity } = usePoll(['admin', 'status'], () => client.admin.status(), 3000);

  const openAdd = async () => {
    if (await AddLibraryModal.call()) {
      await reload();
    }
  };
  const openManage = (lib: AdminLibrary) => {
    void navigate({ to: '/admin/libraries/$id', params: { id: lib.id } });
  };

  const libraries = data?.libraries ?? [];
  const defaults = data?.scrapeDefaults?.languagePriority ?? [];

  return (
    <>
      <PageHeader.Root>
        <PageHeader.Title>{t('admin.librariesTitle')}</PageHeader.Title>
        <PageHeader.Subtitle>{t('admin.librariesSub')}</PageHeader.Subtitle>
        <PageHeader.Actions>
          {canScrape ? (
            <Button
              variant="ghost"
              icon="language"
              label={t('admin.scrapeTitle')}
              onPress={() => void navigate({ to: '/admin/scrape' })}
            />
          ) : null}
          <Button
            variant="primary"
            icon="plus"
            label={t('admin.addLibrary')}
            onPress={() => void openAdd()}
          />
        </PageHeader.Actions>
      </PageHeader.Root>

      {activity ? <LibraryActivity activity={activity} /> : null}

      {data === null ? <TableSkeleton rows={4} /> : null}

      {libraries.length > 0 ? (
        <Box mt={24}>
          <Grid min={360} gap={16}>
            {libraries.map((l) => (
              <LibraryCard
                key={l.id}
                lib={l}
                defaults={defaults}
                scanning={activity?.scanning ?? false}
                onManage={() => openManage(l)}
              />
            ))}
          </Grid>
        </Box>
      ) : null}
      {data && libraries.length === 0 ? (
        <EmptyState.Root icon="library">
          <EmptyState.Title>{t('admin.noLibraries')}</EmptyState.Title>
          <EmptyState.Actions>
            <Button
              variant="primary"
              icon="plus"
              label={t('admin.addLibrary')}
              onPress={() => void openAdd()}
            />
          </EmptyState.Actions>
        </EmptyState.Root>
      ) : null}
    </>
  );
}

/** What the server is busy with right now: a scan has no done/total (the walk
 *  discovers as it goes), enrichment and probing do. */
function LibraryActivity({ activity }: Readonly<{ activity: Activity }>) {
  const t = useT();
  const enriching = activity.enrichTotal > 0 && activity.enrichDone < activity.enrichTotal;
  const probing = activity.phase === 'probing' && activity.probeTotal > 0;
  if (!activity.scanning && !enriching && !probing) {
    return null;
  }
  let title = t('admin.activityScanning');
  let done = 0;
  let total = 0;
  let current: string | null = null;
  let failed = 0;
  if (enriching) {
    title = t('admin.activityEnriching');
    done = activity.enrichDone;
    total = activity.enrichTotal;
    current = activity.enrichCurrent ?? null;
    failed = activity.enrichFailed ?? 0;
  } else if (probing) {
    title = t('admin.activityProbing');
    done = activity.probeDone;
    total = activity.probeTotal;
  }
  return (
    <Surface elevated pad="none" radius="xl" border="border" mt={20} px={20} py={16}>
      <Row between gap={16}>
        <Text variant="meta" color="accentText">
          {title}
        </Text>
        {total > 0 ? (
          <Text variant="meta" color="textMuted">
            {done}/{total}
          </Text>
        ) : null}
      </Row>
      {total > 0 ? (
        <Box mt={10}>
          <Progress value={done / total} rounded />
        </Box>
      ) : null}
      {current ? (
        <Text variant="meta" color="textDim" mt={8} lines={1} minW={0}>
          {t('admin.activityCurrent', { title: current })}
        </Text>
      ) : null}
      {failed > 0 ? (
        <Text variant="meta" color="danger" mt={4}>
          {t('admin.activityFailed', { count: failed })}
        </Text>
      ) : null}
    </Surface>
  );
}

function LibraryCard({
  lib,
  defaults,
  scanning,
  onManage,
}: Readonly<{
  lib: AdminLibrary;
  defaults: string[];
  scanning: boolean;
  onManage: () => void;
}>) {
  const t = useT();
  const fmt = useFormat();
  const { client } = useAuth();
  const [starting, setStarting] = useState(false);

  async function scan() {
    setStarting(true);
    try {
      await client.library.scan(lib.id);
    } finally {
      setTimeout(() => setStarting(false), 2500);
    }
  }

  const glyph = ICONS[lib.kind] ?? 'movie';
  const cover = imageUrl(lib.cover);
  const busyScan = scanning || starting;
  const priority = lib.scrape.languagePriority ?? defaults;
  const language = priority.map((tag) => langName(t, tag) ?? tag).join(' → ');

  return (
    <Surface elevated pad="none" radius="xl" border="border" overflow="hidden">
      <Row gap={14} px={20} py={18} bg="success/7">
        {cover ? (
          <Box w={46} h={46} shrink={0} radius="sm" overflow="hidden" bg="success/16">
            <img
              src={cover}
              alt=""
              style={{ width: '100%', height: '100%', objectFit: 'cover', display: 'block' }}
            />
          </Box>
        ) : (
          <Row center w={46} h={46} shrink={0} radius="sm" bg="success/16">
            <Icon name={glyph} size={22} thickness={1.8} color="success" />
          </Row>
        )}
        <Box flex minW={0}>
          <Text variant="title">{lib.name}</Text>
          <Text variant="meta" color="textDim">
            {t(KIND_LABEL[normalizeLibKind(lib.kind)])} ·{' '}
            {t('admin.itemsCount', { count: lib.itemCount })}
          </Text>
          <Text variant="meta" color="textDim" lines={1}>
            {lib.scrape.languagePriority?.length
              ? t('admin.overridden')
              : t('admin.followGlobal')}
            {language ? ` · ${language}` : ''}
          </Text>
        </Box>
        {lib.autoScan ? (
          <Pill ink="success" bg="success/13">
            {t('admin.autoScanBadge')}
          </Pill>
        ) : null}
      </Row>
      <Divider color="tint/5" />

      <Box row align="stretch">
        <Stat label={t('admin.statSize')} value={fmt.bytes(lib.sizeBytes)} border />
        <Stat label={t('admin.statLastScan')} value={fmt.elapsed(lib.lastScan)} />
      </Box>
      <Divider color="tint/5" />

      <Box px={20} pt={14} pb={16}>
        <Text variant="overline" color="textDim" mb={8}>
          {t('admin.scannedFolders')}
        </Text>
        <Box gap={6}>
          {lib.folders.map((path) => (
            <Row key={path} gap={8}>
              <Icon name="folder" size={15} thickness={1.8} color="success" />
              <Text variant="meta" color="textMuted" lines={1} minW={0}>
                {path}
              </Text>
            </Row>
          ))}
          {lib.folders.length === 0 ? (
            <Text variant="meta" color="textDim">
              {t('admin.noFolders')}
            </Text>
          ) : null}
        </Box>
      </Box>
      <Divider color="tint/5" />

      <Row gap={10} px={20} py={14}>
        <Button
          size="sm"
          icon="refresh"
          label={busyScan ? t('admin.scanning') : t('admin.scan')}
          onPress={() => void scan()}
          loading={busyScan}
          disabled={scanning}
        />
        <Button variant="glass" size="sm" label={t('common.manage')} onPress={onManage} />
      </Row>
    </Surface>
  );
}

function Stat({
  label,
  value,
  border,
}: Readonly<{ label: string; value: string; border?: boolean }>) {
  return (
    <>
      <Box flex px={20} py={14}>
        <DataField.Root size="sm">
          <DataField.Label>{label}</DataField.Label>
          <DataField.Value>{value}</DataField.Value>
        </DataField.Root>
      </Box>
      {border ? <Divider vertical color="tint/5" /> : null}
    </>
  );
}
