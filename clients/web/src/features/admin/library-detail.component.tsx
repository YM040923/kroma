import type { AdminLibrary } from '@kromatv/client/library';
import { useFormat, useT } from '@kromatv/ui';
import {
  Box,
  Button,
  EmptyState,
  Field,
  Icon,
  Row,
  Surface,
  Switch,
  Text,
  useConfirm,
} from '@kromatv/ui/kit';
import { useQuery } from '@tanstack/react-query';
import { useNavigate } from '@tanstack/react-router';
import { useState } from 'react';
import { ArtworkButton } from '#web/features/catalog/artwork-picker.component';
import { LanguagePriorityEditor } from '#web/features/admin/scrape-language.component';
import { Denied, PageHeader, useCap } from '#web/features/admin/shell';
import { FolderListEditor } from '#web/shared/ui/folder-field.component';
import { RouteLink } from '#web/shared/ui/route-link.component';
import { useAuth } from '#web/shared/lib/auth';

const KIND_LABEL = {
  '': 'admin.typeAuto',
  movies: 'admin.typeMovies',
  shows: 'admin.typeShows',
  mixed: 'admin.typeMixed',
  photos: 'admin.typePhotos',
  other: 'admin.typeOther',
} as const;

function normalizedKind(kind: string): keyof typeof KIND_LABEL {
  if (kind === 'shows' || kind === 'tv') {
    return 'shows';
  }
  if (kind === 'movies' || kind === 'film') {
    return 'movies';
  }
  if (kind === 'photos' || kind === 'photo') {
    return 'photos';
  }
  if (kind === 'other' || kind === 'mixed') {
    return kind;
  }
  return '';
}

export function LibraryDetailScreen({ id }: Readonly<{ id: string }>) {
  if (!useCap('library.manage')) {
    return <Denied />;
  }
  return <LibraryDetailInner id={id} />;
}

function LibraryDetailInner({ id }: Readonly<{ id: string }>) {
  const t = useT();
  const { client } = useAuth();
  const query = useQuery({
    queryKey: ['admin', 'libraries'],
    queryFn: () => client.library.list(),
  });
  if (query.isPending) {
    return null;
  }
  const data = query.data;
  const lib = data?.libraries.find((held) => held.id === id);
  if (!lib) {
    return (
      <EmptyState.Root icon="library">
        <EmptyState.Title>{t('admin.libraryNotFound')}</EmptyState.Title>
        <EmptyState.Actions>
          <RouteLink to="/admin/libraries">
            <Text variant="label" color="accentText">
              {t('admin.backToLibraries')}
            </Text>
          </RouteLink>
        </EmptyState.Actions>
      </EmptyState.Root>
    );
  }
  return (
    <LibraryDetail
      key={lib.id}
      lib={lib}
      defaults={data?.scrapeDefaults?.languagePriority ?? []}
      languages={data?.languages ?? []}
      onChanged={() => void query.refetch()}
    />
  );
}

function LibraryDetail({
  lib,
  defaults,
  languages,
  onChanged,
}: Readonly<{
  lib: AdminLibrary;
  defaults: string[];
  languages: string[];
  onChanged: () => void;
}>) {
  const t = useT();
  const fmt = useFormat();
  const { client } = useAuth();
  const confirm = useConfirm();
  const navigate = useNavigate();
  const [name, setName] = useState(lib.name);
  const [folders, setFolders] = useState(lib.folders);
  const [autoScan, setAutoScan] = useState(lib.autoScan);
  const [priority, setPriority] = useState<string[] | null>(
    lib.scrape.languagePriority ?? null,
  );
  const [nfo, setNfo] = useState(lib.scrape.nfo);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [pending, setPending] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [scanning, setScanning] = useState(false);

  const markSaved = () => {
    setSaved(true);
    window.setTimeout(() => setSaved(false), 1600);
  };

  const update = async (patch: Parameters<typeof client.library.update>[1]) => {
    setSaving(true);
    try {
      await client.library.update(lib.id, patch);
      markSaved();
      onChanged();
    } finally {
      setSaving(false);
    }
  };

  const persistScrape = async (next: string[] | null, nextNfo: boolean) => {
    setSaving(true);
    try {
      await client.library.update(lib.id, {
        scrape: {
          ...lib.scrape,
          language: next?.[0] ?? null,
          languagePriority: next,
          nfo: nextNfo,
        },
      });
      setPending(true);
      markSaved();
      onChanged();
    } finally {
      setSaving(false);
    }
  };

  const changeLanguage = (next: string[]) => {
    const value = next.length > 0 ? next : null;
    setPriority(value);
    void persistScrape(value, nfo);
  };

  const changeNfo = (next: boolean) => {
    setNfo(next);
    void persistScrape(priority, next);
  };

  const effective = priority ?? defaults;
  const overridden = priority !== null && priority.length > 0;
  const kind = normalizedKind(lib.kind);

  const remove = async () => {
    const ok = await confirm({
      title: t('common.delete'),
      message: t('admin.confirmDeleteLibrary', { name: lib.name }),
      confirmLabel: t('common.delete'),
      cancelLabel: t('common.cancel'),
      destructive: true,
    });
    if (!ok) {
      return;
    }
    await client.library.delete(lib.id);
    void navigate({ to: '/admin/libraries' });
  };

  const refresh = async () => {
    setRefreshing(true);
    try {
      await client.library.refreshMetadata(lib.id);
    } finally {
      setRefreshing(false);
      setPending(false);
    }
  };

  const scan = async () => {
    setScanning(true);
    try {
      await client.library.scan(lib.id);
    } finally {
      window.setTimeout(() => setScanning(false), 2500);
    }
  };

  return (
    <>
      <RouteLink to="/admin/libraries">
        <Text variant="meta" color="textDim">
          ← {t('admin.backToLibraries')}
        </Text>
      </RouteLink>
      <PageHeader.Root>
        <PageHeader.Title>{lib.name}</PageHeader.Title>
        <PageHeader.Subtitle>
          {t(KIND_LABEL[kind])} · {t('admin.itemsCount', { count: lib.itemCount })} ·{' '}
          {fmt.elapsed(lib.lastScan)}
        </PageHeader.Subtitle>
        <PageHeader.Actions>
          <Button
            variant="glass"
            icon="refresh"
            label={scanning ? t('admin.scanning') : t('admin.scan')}
            loading={scanning}
            onPress={() => void scan()}
          />
        </PageHeader.Actions>
      </PageHeader.Root>

      <Box gap={20} mt={24}>
        <Surface pad="none" radius="xl" border="border">
          <Box px={20} py={18} gap={18}>
            <Text variant="title">{t('admin.basicInfo')}</Text>
            <Field.Root label={t('admin.name')}>
              <Field.Input
                icon="tag"
                value={name}
                onValueChange={setName}
                onBlur={() => {
                  const next = name.trim();
                  if (next && next !== lib.name) {
                    void update({ name: next });
                  }
                }}
              />
            </Field.Root>
            <Row gap={8} align="center">
              <Icon name="lock" size={16} thickness={1.8} color="textDim" />
              <Text variant="label">{t(KIND_LABEL[kind])}</Text>
              <Text variant="meta" color="textDim">
                {t('admin.typeLocked')}
              </Text>
            </Row>
            <Field.Root label={t('admin.scannedFolders')}>
              <FolderListEditor
                folders={folders}
                onChange={(next) => {
                  setFolders(next);
                  void update({ folders: next });
                }}
              />
            </Field.Root>
            <Row between gap={16}>
              <Box>
                <Text variant="label">{t('admin.autoScan')}</Text>
                <Text variant="meta" color="textDim" mt={2}>
                  {t('admin.autoScanHint')}
                </Text>
              </Box>
              <Switch
                checked={autoScan}
                onCheckedChange={(next) => {
                  setAutoScan(next);
                  void update({ autoScan: next });
                }}
                label={t('admin.autoScan')}
              />
            </Row>
            <Row between gap={16}>
              <Box>
                <Text variant="label">{t('admin.libraryCover')}</Text>
                <Text variant="meta" color="textDim" mt={2}>
                  {t('admin.libraryCoverHint')}
                </Text>
              </Box>
              <ArtworkButton itemId={lib.id} title={lib.name} />
            </Row>
            <Row>
              <Button
                variant="dangerGhost"
                size="sm"
                label={t('common.delete')}
                onPress={() => void remove()}
              />
            </Row>
          </Box>
        </Surface>

        <Surface pad="none" radius="xl" border="border">
          <Box px={20} py={18} gap={18}>
            <Row between gap={16} wrap>
              <Row gap={10} align="center">
                <Text variant="title">{t('admin.scrapeSettings')}</Text>
                <Text variant="meta" color={overridden ? 'accentText' : 'textDim'}>
                  {overridden
                    ? t('admin.overridden')
                    : t('admin.followGlobal')}
                </Text>
              </Row>
              <Row gap={10} align="center">
                {saving ? (
                  <Text variant="meta" color="textDim">
                    {t('common.saving')}
                  </Text>
                ) : saved ? (
                  <Text variant="meta" color="success">
                    {t('admin.scrapeSaved')}
                  </Text>
                ) : null}
                {overridden ? (
                  <Button
                    variant="ghost"
                    size="sm"
                    label={t('admin.followAction')}
                    onPress={() => changeLanguage([])}
                  />
                ) : null}
              </Row>
            </Row>

            <Box gap={10}>
              <Text variant="label">{t('admin.scrapeLanguageTitle')}</Text>
              <Text variant="meta" color="textDim">
                {t('admin.scrapeLanguageDesc')}
              </Text>
              <LanguagePriorityEditor
                value={effective}
                languages={languages}
                onChange={changeLanguage}
              />
            </Box>

            <Row between gap={16}>
              <Box>
                <Text variant="label">{t('admin.nfoFiles')}</Text>
                <Text variant="meta" color="textDim" mt={2}>
                  {t('admin.nfoFilesHint')}
                </Text>
              </Box>
              <Switch checked={nfo} onCheckedChange={changeNfo} label={t('admin.nfoFiles')} />
            </Row>

            {pending ? (
              <Surface pad="none" radius="lg" border="border" px={16} py={12}>
                <Row between gap={12} wrap>
                  <Text variant="meta" color="textMuted" flex>
                    {t('admin.scrapePending')}
                  </Text>
                  <Button
                    variant="glass"
                    size="sm"
                    icon="refresh"
                    label={
                      refreshing ? t('admin.refreshing') : t('admin.refreshMetadata')
                    }
                    disabled={refreshing}
                    onPress={() => void refresh()}
                  />
                </Row>
              </Surface>
            ) : null}
          </Box>
        </Surface>
      </Box>
    </>
  );
}