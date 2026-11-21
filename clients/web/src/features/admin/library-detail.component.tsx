import type { AdminLibrary, ScrapeDefaults } from '@kromatv/client/library';
import { useFormat, useT } from '@kromatv/ui';
import {
  Box,
  Button,
  EmptyState,
  Field,
  Icon,
  Row,
  SegmentGroup,
  Surface,
  Switch,
  Text,
  useConfirm,
} from '@kromatv/ui/kit';
import { useQuery } from '@tanstack/react-query';
import { useNavigate } from '@tanstack/react-router';
import { useState } from 'react';
import { ArtworkButton } from '#web/features/catalog/artwork-picker.component';
import { ArtRuleEditor, type ArtRuleValue } from '#web/features/admin/scrape-art.component';
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

function messageOf(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason);
}

interface ScrapeDraft {
  priority: string[] | null;
  nfo: boolean | null;
  posterLanguages: string[] | null;
  posterMinWidth: number | null;
  posterMinVotes: number | null;
  posterSize: string | null;
  backdropLanguages: string[] | null;
  backdropMinWidth: number | null;
  backdropMinVotes: number | null;
  backdropSize: string | null;
  logoLanguages: string[] | null;
  logoMinWidth: number | null;
  logoSize: string | null;
  seasonLanguages: string[] | null;
  seasonMinWidth: number | null;
  seasonSize: string | null;
  posterSources: string[] | null;
  backdropSources: string[] | null;
  logoSources: string[] | null;
  seasonSources: string[] | null;
}

const FOLLOW_GLOBAL_DRAFT: ScrapeDraft = {  priority: null,
  nfo: null,
  posterLanguages: null,
  posterMinWidth: null,
  posterMinVotes: null,
  posterSize: null,
  backdropLanguages: null,
  backdropMinWidth: null,
  backdropMinVotes: null,
  backdropSize: null,
  logoLanguages: null,
  logoMinWidth: null,
  logoSize: null,
  seasonLanguages: null,
  seasonMinWidth: null,
  seasonSize: null,
  posterSources: null,
  backdropSources: null,
  logoSources: null,
  seasonSources: null,
};

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
    // Global scraping defaults live in the same response; a library page opened
    // after the global page changed must not render the cached old rules.
    refetchOnMount: 'always',
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
      defaults={data?.scrapeDefaults ?? { languagePriority: [] }}
      languages={data?.languages ?? []}
      fanartConfigured={data?.fanartConfigured === true}
      onChanged={() => void query.refetch()}
    />
  );
}

function LibraryDetail({
  lib,
  defaults,
  languages,
  fanartConfigured,
  onChanged,
}: Readonly<{
  lib: AdminLibrary;
  defaults: ScrapeDefaults;
  languages: string[];
  fanartConfigured: boolean;
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
  const [acquisitionMode, setAcquisitionMode] = useState(lib.acquisitionMode);
  const [draft, setDraft] = useState<ScrapeDraft>(() => ({
    priority: lib.scrape.languagePriority ?? null,
    nfo: lib.scrape.nfoOverride ?? null,
    posterLanguages: lib.scrape.posterLanguages ?? null,
    posterMinWidth: lib.scrape.posterMinWidth ?? null,
    posterMinVotes: lib.scrape.posterMinVotes ?? null,
    posterSize: lib.scrape.posterSize ?? null,
    backdropLanguages: lib.scrape.backdropLanguages ?? null,
    backdropMinWidth: lib.scrape.backdropMinWidth ?? null,
    backdropMinVotes: lib.scrape.backdropMinVotes ?? null,
    backdropSize: lib.scrape.backdropSize ?? null,
    logoLanguages: lib.scrape.logoLanguages ?? null,
    logoMinWidth: lib.scrape.logoMinWidth ?? null,
    logoSize: lib.scrape.logoSize ?? null,
    seasonLanguages: lib.scrape.seasonLanguages ?? null,
    seasonMinWidth: lib.scrape.seasonMinWidth ?? null,
    seasonSize: lib.scrape.seasonSize ?? null,
    posterSources: lib.scrape.posterSources ?? null,
    backdropSources: lib.scrape.backdropSources ?? null,
    logoSources: lib.scrape.logoSources ?? null,
    seasonSources: lib.scrape.seasonSources ?? null,
  }));
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [customOpen, setCustomOpen] = useState(false);
  const [pending, setPending] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [scanning, setScanning] = useState(false);

  const markSaved = () => {
    setSaved(true);
    window.setTimeout(() => setSaved(false), 1600);
  };

  const update = async (patch: Parameters<typeof client.library.update>[1]) => {
    setSaving(true);
    setError(null);
    try {
      await client.library.update(lib.id, patch);
      markSaved();
      onChanged();
    } catch (reason) {
      setError(messageOf(reason));
    } finally {
      setSaving(false);
    }
  };

  const persistScrape = async (next: ScrapeDraft): Promise<boolean> => {
    setDraft(next);
    setSaving(true);
    setError(null);
    try {
      await client.library.update(lib.id, {
        scrape: {
          ...lib.scrape,
          language: next.priority?.[0] ?? null,
          languagePriority: next.priority,
          nfo: next.nfo ?? false,
          nfoOverride: next.nfo,
          posterLanguages: next.posterLanguages,
          posterMinWidth: next.posterMinWidth,
          posterMinVotes: next.posterMinVotes,
          posterSize: next.posterSize,
          backdropLanguages: next.backdropLanguages,
          backdropMinWidth: next.backdropMinWidth,
          backdropMinVotes: next.backdropMinVotes,
          backdropSize: next.backdropSize,
          logoLanguages: next.logoLanguages,
          logoMinWidth: next.logoMinWidth,
          logoSize: next.logoSize,
          seasonLanguages: next.seasonLanguages,
          seasonMinWidth: next.seasonMinWidth,
          seasonSize: next.seasonSize,
          posterSources: next.posterSources,
          backdropSources: next.backdropSources,
          logoSources: next.logoSources,
          seasonSources: next.seasonSources,
        },
      });
      setPending(true);
      markSaved();
      onChanged();
      return true;
    } catch (reason) {
      setError(messageOf(reason));
      return false;
    } finally {
      setSaving(false);
    }
  };

  const changeLanguage = (next: string[]) => {
    void persistScrape({ ...draft, priority: next.length > 0 ? next : null });
  };

  const followGlobal = async () => {
    const ok = await confirm({
      title: t('admin.followGlobal'),
      message: t('admin.scrapeFollowConfirm'),
      confirmLabel: t('admin.followGlobal'),
      cancelLabel: t('common.cancel'),
      destructive: true,
    });
    if (!ok) {
      return;
    }
    const kept = await persistScrape({ ...FOLLOW_GLOBAL_DRAFT });
    setCustomOpen(!kept);
  };

  const changeNfo = (next: boolean | null) => {
    void persistScrape({ ...draft, nfo: next });
  };

  const effective = draft.priority ?? defaults.languagePriority;
  const overridden = draft.priority !== null && draft.priority.length > 0;
  const nfo = draft.nfo ?? defaults.nfo ?? false;
  const nfoOverridden = draft.nfo !== null;
  const sourceProps = (
    key: 'posterSources' | 'backdropSources' | 'logoSources' | 'seasonSources',
    fallback: string[],
  ) =>
    fanartConfigured
      ? {
          value: draft[key] ?? defaults[key] ?? fallback,
          onChange: (next: string[]) => void persistScrape({ ...draft, [key]: next }),
        }
      : undefined;
  const poster: ArtRuleValue = {
    languages: draft.posterLanguages ?? defaults.posterLanguages ?? null,
    minWidth: draft.posterMinWidth ?? defaults.posterMinWidth ?? null,
    minVotes: draft.posterMinVotes ?? defaults.posterMinVotes ?? null,
    size: draft.posterSize ?? defaults.posterSize ?? null,
  };
  const backdrop: ArtRuleValue = {
    languages: draft.backdropLanguages ?? defaults.backdropLanguages ?? null,
    minWidth: draft.backdropMinWidth ?? defaults.backdropMinWidth ?? null,
    minVotes: draft.backdropMinVotes ?? defaults.backdropMinVotes ?? null,
    size: draft.backdropSize ?? defaults.backdropSize ?? null,
  };
  const posterOverridden =
    draft.posterLanguages !== null ||
    draft.posterMinWidth !== null ||
    draft.posterMinVotes !== null ||
    draft.posterSize !== null ||
    draft.posterSources !== null;
  const logo: ArtRuleValue = {
    languages: draft.logoLanguages ?? defaults.logoLanguages ?? null,
    minWidth: draft.logoMinWidth ?? defaults.logoMinWidth ?? null,
    minVotes: null,
    size: draft.logoSize ?? defaults.logoSize ?? null,
  };
  const backdropOverridden =
    draft.backdropLanguages !== null ||
    draft.backdropMinWidth !== null ||
    draft.backdropMinVotes !== null ||
    draft.backdropSize !== null ||
    draft.backdropSources !== null;
  const season: ArtRuleValue = {
    languages: draft.seasonLanguages ?? defaults.seasonLanguages ?? null,
    minWidth: draft.seasonMinWidth ?? defaults.seasonMinWidth ?? null,
    minVotes: null,
    size: draft.seasonSize ?? defaults.seasonSize ?? null,
  };
  const logoOverridden =
    draft.logoLanguages !== null ||
    draft.logoMinWidth !== null ||
    draft.logoSize !== null ||
    draft.logoSources !== null;
  const seasonOverridden =
    draft.seasonLanguages !== null ||
    draft.seasonMinWidth !== null ||
    draft.seasonSize !== null ||
    draft.seasonSources !== null;
  const anyOverridden =
    overridden ||
    nfoOverridden ||
    posterOverridden ||
    backdropOverridden ||
    logoOverridden ||
    seasonOverridden;
  const showCustom = anyOverridden || customOpen;
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
      <PageHeader.Root>
        <PageHeader.Back label={t('admin.backToLibraries')} asChild>
          <RouteLink to="/admin/libraries" />
        </PageHeader.Back>
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

      {error ? (
        <Text variant="meta" color="danger" mt={12}>
          {error}
        </Text>
      ) : null}

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

        {['film', 'mixed'].includes(lib.kind) ? (
          <Surface pad="none" radius="xl" border="border">
            <Box px={20} py={18} gap={12}>
              <Box>
                <Text variant="title">{t('admin.acquisitionPlacement')}</Text>
                <Text variant="meta" color="textDim" mt={2}>
                  {t('admin.acquisitionPlacementHint')}
                </Text>
              </Box>
              <SegmentGroup.Root
                value={acquisitionMode}
                onValueChange={(next) => {
                  if (
                    next !== 'cloud_only_staged' &&
                    next !== 'hybrid_local_first' &&
                    next !== 'local_only'
                  ) {
                    return;
                  }
                  setAcquisitionMode(next);
                  void update({ acquisitionMode: next });
                }}
                label={t('admin.acquisitionPlacement')}
              >
                <SegmentGroup.Item value="cloud_only_staged">
                  <SegmentGroup.Label>{t('admin.acquisitionCloudStaged')}</SegmentGroup.Label>
                </SegmentGroup.Item>
                <SegmentGroup.Item value="hybrid_local_first">
                  <SegmentGroup.Label>{t('admin.acquisitionHybridLocalFirst')}</SegmentGroup.Label>
                </SegmentGroup.Item>
                <SegmentGroup.Item value="local_only">
                  <SegmentGroup.Label>{t('admin.acquisitionLocalOnly')}</SegmentGroup.Label>
                </SegmentGroup.Item>
              </SegmentGroup.Root>
              {saving ? (
                <Text variant="meta" color="textDim">{t('common.saving')}</Text>
              ) : saved ? (
                <Text variant="meta" color="success">{t('admin.scrapeSaved')}</Text>
              ) : error ? (
                <Text variant="meta" color="danger">{error}</Text>
              ) : null}
            </Box>
          </Surface>
        ) : null}

        <Surface pad="none" radius="xl" border="border">
          <Box px={20} py={18} gap={18}>
            <Row between gap={16} wrap>
              <Box>
                <Text variant="title">{t('admin.scrapeSettings')}</Text>
                <Text variant="meta" color="textDim" mt={2}>
                  {showCustom
                    ? anyOverridden
                      ? t('admin.scrapeCustomHint')
                      : t('admin.scrapeCustomDraft')
                    : t('admin.scrapeFollowHint')}
                </Text>
              </Box>
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
                <SegmentGroup.Root
                  value={showCustom ? 'custom' : 'global'}
                  onValueChange={(next) => {
                    if (next === 'custom') {
                      setCustomOpen(true);
                    } else {
                      void followGlobal();
                    }
                  }}
                  label={t('admin.scrapeSettings')}
                >
                  <SegmentGroup.Item value="global">
                    <SegmentGroup.Label>{t('admin.followGlobal')}</SegmentGroup.Label>
                  </SegmentGroup.Item>
                  <SegmentGroup.Item value="custom">
                    <SegmentGroup.Label>{t('admin.scrapeCustom')}</SegmentGroup.Label>
                  </SegmentGroup.Item>
                </SegmentGroup.Root>
              </Row>
            </Row>

            {showCustom ? (
              <>
            <Box gap={10}>
              <Row between gap={12}>
                <Box>
                  <Text variant="label">{t('admin.scrapeLanguageTitle')}</Text>
                  <Text variant="meta" color="textDim" mt={2}>
                    {t('admin.scrapeLanguageDesc')}
                  </Text>
                </Box>
                {overridden ? (
                  <Button
                    variant="ghost"
                    size="sm"
                    label={t('admin.followAction')}
                    onPress={() => changeLanguage([])}
                  />
                ) : null}
              </Row>
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
              <Row gap={8} align="center">
                {nfoOverridden ? (
                  <Button
                    variant="ghost"
                    size="sm"
                    label={t('admin.followAction')}
                    onPress={() => changeNfo(null)}
                  />
                ) : null}
                <Switch
                  checked={nfo}
                  onCheckedChange={changeNfo}
                  label={t('admin.nfoFiles')}
                />
              </Row>
            </Row>

            <ArtRuleEditor
              title={t('admin.posterPreference')}
              hint={t('admin.posterRuleHint')}
              value={poster}
              languages={languages}
              widthOptions={[300, 500, 780, 1000]}
              voteOptions={[10, 50, 100, 500]}
              sources={sourceProps('posterSources', ['tmdb', 'fanart'])}
              onChange={(patch) =>
                void persistScrape({
                  ...draft,
                  posterLanguages:
                    patch.languages !== undefined
                      ? patch.languages
                      : draft.posterLanguages,
                  posterMinWidth:
                    patch.minWidth !== undefined ? patch.minWidth : draft.posterMinWidth,
                  posterMinVotes:
                    patch.minVotes !== undefined ? patch.minVotes : draft.posterMinVotes,
                  posterSize:
                    patch.size !== undefined ? patch.size : draft.posterSize,
                })
              }
            />

            <ArtRuleEditor
              title={t('admin.backdropPreference')}
              hint={t('admin.backdropRuleHint')}
              value={backdrop}
              languages={languages}
              widthOptions={[1280, 1920, 2560]}
              voteOptions={[10, 50, 100, 500]}
              sources={sourceProps('backdropSources', ['tmdb', 'fanart'])}
              onChange={(patch) =>
                void persistScrape({
                  ...draft,
                  backdropLanguages:
                    patch.languages !== undefined
                      ? patch.languages
                      : draft.backdropLanguages,
                  backdropMinWidth:
                    patch.minWidth !== undefined
                      ? patch.minWidth
                      : draft.backdropMinWidth,
                  backdropMinVotes:
                    patch.minVotes !== undefined
                      ? patch.minVotes
                      : draft.backdropMinVotes,
                  backdropSize:
                    patch.size !== undefined ? patch.size : draft.backdropSize,
                })
              }
            />

            <ArtRuleEditor
              title={t('artwork.logo')}
              hint={t('admin.logoRuleHint')}
              value={logo}
              languages={languages}
              widthOptions={[200, 300, 500]}
              sources={sourceProps('logoSources', ['fanart', 'tmdb'])}
              onChange={(patch) =>
                void persistScrape({
                  ...draft,
                  logoLanguages:
                    patch.languages !== undefined
                      ? patch.languages
                      : draft.logoLanguages,
                  logoMinWidth:
                    patch.minWidth !== undefined ? patch.minWidth : draft.logoMinWidth,
                  logoSize: patch.size !== undefined ? patch.size : draft.logoSize,
                })
              }
            />

            {kind === 'shows' || kind === 'mixed' ? (
              <ArtRuleEditor
                title={t('admin.seasonPoster')}
                hint={t('admin.seasonRuleHint')}
                value={season}
                languages={languages}
                widthOptions={[300, 500, 780]}
                sources={sourceProps('seasonSources', ['fanart', 'tmdb'])}
                onChange={(patch) =>
                  void persistScrape({
                    ...draft,
                    seasonLanguages:
                      patch.languages !== undefined
                        ? patch.languages
                        : draft.seasonLanguages,
                    seasonMinWidth:
                      patch.minWidth !== undefined
                        ? patch.minWidth
                        : draft.seasonMinWidth,
                    seasonSize:
                      patch.size !== undefined ? patch.size : draft.seasonSize,
                  })
                }
              />
            ) : null}
              </>
            ) : null}

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