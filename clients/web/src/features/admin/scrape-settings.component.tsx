import type { ScrapeDefaults } from '@kromatv/client/library';
import { useT } from '@kromatv/ui';
import { Badge, Box, Button, Field, NumberField, Row, Surface, Switch, Text, useConfirm } from '@kromatv/ui/kit';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { ArtRuleEditor, type ArtRuleValue } from '#web/features/admin/scrape-art.component';
import { LanguagePriorityEditor } from '#web/features/admin/scrape-language.component';
import { Denied, PageHeader, useCap } from '#web/features/admin/shell';
import { useAuth } from '#web/shared/lib/auth';

export function ScrapeSettingsScreen() {
  if (!useCap('settings.manage')) {
    return <Denied />;
  }
  return <ScrapeSettingsInner />;
}

function messageOf(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason);
}

function ScrapeSettingsInner() {
  const t = useT();
  const { client } = useAuth();
  const query = useQuery({
    queryKey: ['admin', 'scrape'],
    queryFn: () => client.library.scrapeDefaults(),
  });
  const queryClient = useQueryClient();
  const confirm = useConfirm();
  const canManageLibraries = useCap('library.manage');
  const [draft, setDraft] = useState<Partial<ScrapeDefaults>>({});
  const [saved, setSaved] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [applying, setApplying] = useState(false);
  const [keyBusy, setKeyBusy] = useState(false);
  const [fanartKey, setFanartKey] = useState('');
  const [fanartSaved, setFanartSaved] = useState(false);

  if (query.isPending) {
    return null;
  }
  const data = query.data;
  if (!data) {
    return null;
  }
  const value: ScrapeDefaults = {
    languagePriority: draft.languagePriority ?? data.languagePriority,
    nfo: draft.nfo ?? data.nfo ?? false,
    posterLanguages:
      draft.posterLanguages !== undefined ? draft.posterLanguages : (data.posterLanguages ?? null),
    backdropLanguages:
      draft.backdropLanguages !== undefined
        ? draft.backdropLanguages
        : (data.backdropLanguages ?? null),
    posterMinWidth:
      draft.posterMinWidth !== undefined ? draft.posterMinWidth : (data.posterMinWidth ?? null),
    posterMinVotes:
      draft.posterMinVotes !== undefined ? draft.posterMinVotes : (data.posterMinVotes ?? null),
    backdropMinWidth:
      draft.backdropMinWidth !== undefined
        ? draft.backdropMinWidth
        : (data.backdropMinWidth ?? null),
    backdropMinVotes:
      draft.backdropMinVotes !== undefined
        ? draft.backdropMinVotes
        : (data.backdropMinVotes ?? null),
    posterSize: draft.posterSize !== undefined ? draft.posterSize : (data.posterSize ?? null),
    backdropSize:
      draft.backdropSize !== undefined ? draft.backdropSize : (data.backdropSize ?? null),
    logoLanguages:
      draft.logoLanguages !== undefined ? draft.logoLanguages : (data.logoLanguages ?? null),
    logoMinWidth:
      draft.logoMinWidth !== undefined ? draft.logoMinWidth : (data.logoMinWidth ?? null),
    logoSize: draft.logoSize !== undefined ? draft.logoSize : (data.logoSize ?? null),
    seasonLanguages:
      draft.seasonLanguages !== undefined
        ? draft.seasonLanguages
        : (data.seasonLanguages ?? null),
    seasonMinWidth:
      draft.seasonMinWidth !== undefined ? draft.seasonMinWidth : (data.seasonMinWidth ?? null),
    seasonSize:
      draft.seasonSize !== undefined ? draft.seasonSize : (data.seasonSize ?? null),
    posterSources:
      draft.posterSources !== undefined ? draft.posterSources : (data.posterSources ?? null),
    backdropSources:
      draft.backdropSources !== undefined
        ? draft.backdropSources
        : (data.backdropSources ?? null),
    logoSources:
      draft.logoSources !== undefined ? draft.logoSources : (data.logoSources ?? null),
    seasonSources:
      draft.seasonSources !== undefined ? draft.seasonSources : (data.seasonSources ?? null),
    artworkRefreshDays:
      draft.artworkRefreshDays !== undefined
        ? draft.artworkRefreshDays
        : (data.artworkRefreshDays ?? null),
    writeImages: draft.writeImages ?? data.writeImages ?? false,
  };
  const languages = data.languages ?? [];

  const save = (patch: Partial<ScrapeDefaults>) => {
    const next = { ...value, ...patch };
    setDraft(next);
    setSaveError(null);
    void client.library
      .setScrapeDefaults(next)
      .then(() => {
        setSaved(true);
        window.setTimeout(() => setSaved(false), 1600);
        void query.refetch();
        // The library page reads these defaults through its own query; without
        // this it keeps showing the old rules until a full reload.
        void queryClient.invalidateQueries({ queryKey: ['admin', 'libraries'] });
      })
      .catch((reason) => setSaveError(messageOf(reason) || t('admin.scrapeSaveFailed')));
  };

  const applyAll = async () => {
    const ok = await confirm({
      title: t('admin.scrapeApplyAll'),
      message: t('admin.scrapeApplyAllConfirm'),
      confirmLabel: t('admin.scrapeApplyAll'),
      cancelLabel: t('common.cancel'),
    });
    if (!ok) {
      return;
    }
    setApplying(true);
    setSaveError(null);
    try {
      const list = await client.library.list();
      for (const library of list.libraries) {
        if (!['film', 'tv', 'mixed'].includes(library.kind) || library.itemCount === 0) {
          continue;
        }
        await client.library.refreshMetadata(library.id);
      }
      setSaved(true);
      window.setTimeout(() => setSaved(false), 1600);
      void queryClient.invalidateQueries({ queryKey: ['admin', 'libraries'] });
    } catch (reason) {
      setSaveError(messageOf(reason));
    } finally {
      setApplying(false);
    }
  };

  const poster: ArtRuleValue = {
    languages: value.posterLanguages ?? null,
    minWidth: value.posterMinWidth ?? null,
    minVotes: value.posterMinVotes ?? null,
    size: value.posterSize ?? null,
  };
  const backdrop: ArtRuleValue = {
    languages: value.backdropLanguages ?? null,
    minWidth: value.backdropMinWidth ?? null,
    minVotes: value.backdropMinVotes ?? null,
    size: value.backdropSize ?? null,
  };
  const logo: ArtRuleValue = {
    languages: value.logoLanguages ?? null,
    minWidth: value.logoMinWidth ?? null,
    minVotes: null,
    size: value.logoSize ?? null,
  };
  const season: ArtRuleValue = {
    languages: value.seasonLanguages ?? null,
    minWidth: value.seasonMinWidth ?? null,
    minVotes: null,
    size: value.seasonSize ?? null,
  };
  const fanart = data.fanartConfigured === true;
  const fanartPaused =
    !fanart &&
    [value.posterSources, value.backdropSources, value.logoSources, value.seasonSources].some(
      (list) => list?.length === 1 && list[0] === 'fanart',
    );
  const sourceProps = (
    configured: string[] | null | undefined,
    fallback: string[],
    key: 'posterSources' | 'backdropSources' | 'logoSources' | 'seasonSources',
  ) =>
    fanart
      ? {
          value: configured ?? fallback,
          onChange: (next: string[]) => save({ [key]: next }),
        }
      : undefined;

  return (
    <>
      <PageHeader.Root>
        <PageHeader.Title>{t('admin.scrapeTitle')}</PageHeader.Title>
        <PageHeader.Subtitle>{t('admin.scrapeSub')}</PageHeader.Subtitle>
        <PageHeader.Actions>
          {saved ? (
            <Text variant="meta" color="success">
              {t('admin.scrapeSaved')}
            </Text>
          ) : null}
        </PageHeader.Actions>
      </PageHeader.Root>

      <Box mt={24} gap={20}>
        {saveError ? (
          <Text variant="meta" color="danger">
            {saveError}
          </Text>
        ) : null}
        <Surface pad="none" radius="xl" border="border">
          <Box px={20} py={18} gap={12}>
            <Box>
              <Text variant="title">{t('admin.scrapeLanguageTitle')}</Text>
              <Text variant="meta" color="textDim" mt={4}>
                {t('admin.scrapeLanguageDesc')}
              </Text>
            </Box>
            <LanguagePriorityEditor
              value={value.languagePriority}
              languages={languages}
              min={1}
              onChange={(next) => {
                if (next.length > 0) {
                  save({ languagePriority: next });
                }
              }}
            />
          </Box>
        </Surface>

        <Surface pad="none" radius="xl" border="border">
          <Box px={20} py={18} gap={20}>
            <Box>
              <Text variant="title">{t('admin.artSectionTitle')}</Text>
              <Text variant="meta" color="textDim" mt={4}>
                {t('admin.artSectionHint')}
              </Text>
            </Box>
            <ArtRuleEditor
              title={t('admin.posterPreference')}
              hint={t('admin.posterRuleHint')}
              value={poster}
              languages={languages}
              widthOptions={[300, 500, 780, 1000]}
              voteOptions={[10, 50, 100, 500]}
              sources={sourceProps(value.posterSources, ['tmdb', 'fanart'], 'posterSources')}
              onChange={(patch) =>
                save({
                  posterLanguages:
                    patch.languages !== undefined ? patch.languages : value.posterLanguages,
                  posterMinWidth:
                    patch.minWidth !== undefined ? patch.minWidth : value.posterMinWidth,
                  posterMinVotes:
                    patch.minVotes !== undefined ? patch.minVotes : value.posterMinVotes,
                  posterSize: patch.size !== undefined ? patch.size : value.posterSize,
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
              sources={sourceProps(
                value.backdropSources,
                ['tmdb', 'fanart'],
                'backdropSources',
              )}
              onChange={(patch) =>
                save({
                  backdropLanguages:
                    patch.languages !== undefined ? patch.languages : value.backdropLanguages,
                  backdropMinWidth:
                    patch.minWidth !== undefined ? patch.minWidth : value.backdropMinWidth,
                  backdropMinVotes:
                    patch.minVotes !== undefined ? patch.minVotes : value.backdropMinVotes,
                  backdropSize:
                    patch.size !== undefined ? patch.size : value.backdropSize,
                })
              }
            />
            <ArtRuleEditor
              title={t('artwork.logo')}
              hint={t('admin.logoRuleHint')}
              value={logo}
              languages={languages}
              widthOptions={[200, 300, 500]}
              sources={sourceProps(value.logoSources, ['fanart', 'tmdb'], 'logoSources')}
              onChange={(patch) =>
                save({
                  logoLanguages:
                    patch.languages !== undefined ? patch.languages : value.logoLanguages,
                  logoMinWidth:
                    patch.minWidth !== undefined ? patch.minWidth : value.logoMinWidth,
                  logoSize: patch.size !== undefined ? patch.size : value.logoSize,
                })
              }
            />
            <ArtRuleEditor
              title={t('admin.seasonPoster')}
              hint={t('admin.seasonRuleHint')}
              value={season}
              languages={languages}
              widthOptions={[300, 500, 780]}
              sources={sourceProps(
                value.seasonSources,
                ['fanart', 'tmdb'],
                'seasonSources',
              )}
              onChange={(patch) =>
                save({
                  seasonLanguages:
                    patch.languages !== undefined ? patch.languages : value.seasonLanguages,
                  seasonMinWidth:
                    patch.minWidth !== undefined ? patch.minWidth : value.seasonMinWidth,
                  seasonSize:
                    patch.size !== undefined ? patch.size : value.seasonSize,
                })
              }
            />
          </Box>
        </Surface>

        <Surface pad="none" radius="xl" border="border">
          <Box px={20} py={18} gap={14}>
            <Row between gap={12} wrap>
              <Box>
                <Text variant="title">Fanart.tv</Text>
                <Text variant="meta" color="textDim" mt={4}>
                  {t('admin.fanartHint')}
                </Text>
              </Box>
              <Badge tone={fanart ? 'success' : 'neutral'}>
                {fanart ? t('admin.fanartConfigured') : t('admin.fanartNotConfigured')}
              </Badge>
            </Row>
            <Row wrap gap={12} align="flex-end">
              <Box w={320}>
                <Field.Root
                  label={t('admin.fanartKey')}
                  value={fanartKey}
                  onValueChange={setFanartKey}
                >
                  <Field.Input
                    type="password"
                    placeholder={
                      fanart ? t('admin.fanartKeyKeep') : 'https://fanart.tv 个人 API Key'
                    }
                  />
                </Field.Root>
              </Box>
              <Button
                variant="glass"
                size="sm"
                label={t('admin.fanartSave')}
                disabled={!fanartKey.trim() || keyBusy}
                loading={keyBusy}
                onPress={() => {
                  setSaveError(null);
                  setKeyBusy(true);
                  void client.library
                    .setScrapeDefaults({ fanartApiKey: fanartKey.trim() })
                    .then(() => {
                      setFanartKey('');
                      setFanartSaved(true);
                      window.setTimeout(() => setFanartSaved(false), 1600);
                      void query.refetch();
                      void queryClient.invalidateQueries({ queryKey: ['admin', 'libraries'] });
                    })
                    .catch((reason) => setSaveError(messageOf(reason)))
                    .finally(() => setKeyBusy(false));
                }}
              />
              {fanart ? (
                <Button
                  variant="ghost"
                  size="sm"
                  label={t('admin.fanartClear')}
                  loading={keyBusy}
                  onPress={() => {
                    setSaveError(null);
                    setKeyBusy(true);
                    void client.library
                      .setScrapeDefaults({ clearFanartKey: true })
                      .then(() => {
                        void query.refetch();
                        void queryClient.invalidateQueries({ queryKey: ['admin', 'libraries'] });
                      })
                      .catch((reason) => setSaveError(messageOf(reason)))
                      .finally(() => setKeyBusy(false));
                  }}
                />
              ) : null}
              {fanartSaved ? (
                <Text variant="meta" color="success">
                  {t('admin.scrapeSaved')}
                </Text>
              ) : null}
            </Row>
            {fanartPaused ? (
              <Text variant="meta" color="textDim">
                {t('admin.fanartPaused')}
              </Text>
            ) : null}
          </Box>
        </Surface>

        <Surface pad="none" radius="xl" border="border">
          <Box px={20} py={18} gap={14}>
            <Box>
              <Text variant="title">{t('admin.writeSectionTitle')}</Text>
              <Text variant="meta" color="textDim" mt={4}>
                {t('admin.writeSectionHint')}
              </Text>
            </Box>
            <Row between gap={16}>
              <Box>
                <Text variant="label">{t('admin.writeImages')}</Text>
                <Text variant="meta" color="textDim" mt={2}>
                  {t('admin.writeImagesHint')}
                </Text>
              </Box>
              <Switch
                checked={value.writeImages ?? false}
                onCheckedChange={(writeImages) => save({ writeImages })}
                label={t('admin.writeImages')}
              />
            </Row>
            <Row between gap={16}>
              <Box>
                <Text variant="label">{t('admin.nfoFiles')}</Text>
                <Text variant="meta" color="textDim" mt={2}>
                  {t('admin.nfoFilesHint')}
                </Text>
              </Box>
              <Switch
                checked={value.nfo ?? false}
                onCheckedChange={(nfo) => save({ nfo })}
                label={t('admin.nfoFiles')}
              />
            </Row>
            <Row between gap={16}>
              <Box>
                <Text variant="label">{t('admin.artworkRefresh')}</Text>
                <Text variant="meta" color="textDim" mt={2}>
                  {value.artworkRefreshDays
                    ? t('admin.artworkRefreshDays', { count: value.artworkRefreshDays })
                    : t('admin.artworkRefreshOff')}
                </Text>
              </Box>
              <NumberField
                label={t('admin.artworkRefresh')}
                value={value.artworkRefreshDays ?? 0}
                min={0}
                max={365}
                step={1}
                w={140}
                onValueChange={(days) =>
                  save({ artworkRefreshDays: days > 0 ? days : null })
                }
              />
            </Row>
          </Box>
        </Surface>

        {canManageLibraries ? (
          <Surface pad="none" radius="xl" border="border">
            <Box px={20} py={18} gap={14}>
              <Row between gap={16} wrap>
                <Box>
                  <Text variant="title">{t('admin.scrapeApplyAll')}</Text>
                  <Text variant="meta" color="textDim" mt={4}>
                    {t('admin.scrapeApplyAllHint')}
                  </Text>
                </Box>
                <Button
                  variant="glass"
                  size="sm"
                  icon="refresh"
                  label={t('admin.scrapeApplyAll')}
                  loading={applying}
                  onPress={() => void applyAll()}
                />
              </Row>
            </Box>
          </Surface>
        ) : null}
      </Box>
    </>
  );
}