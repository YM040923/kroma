import type { ScrapeDefaults } from '@kromatv/client/library';
import { useT } from '@kromatv/ui';
import { Badge, Box, Button, Field, Row, Surface, Switch, Text } from '@kromatv/ui/kit';
import { useQuery } from '@tanstack/react-query';
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

function ScrapeSettingsInner() {
  const t = useT();
  const { client } = useAuth();
  const query = useQuery({
    queryKey: ['admin', 'scrape'],
    queryFn: () => client.library.scrapeDefaults(),
  });
  const [draft, setDraft] = useState<Partial<ScrapeDefaults>>({});
  const [saved, setSaved] = useState(false);
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
    posterLanguages:
      draft.posterLanguages !== undefined ? draft.posterLanguages : (data.posterLanguages ?? null),
    backdropLanguages:
      draft.backdropLanguages !== undefined
        ? draft.backdropLanguages
        : (data.backdropLanguages ?? null),
    posterMinWidth:
      draft.posterMinWidth !== undefined ? draft.posterMinWidth : (data.posterMinWidth ?? null),
    backdropMinWidth:
      draft.backdropMinWidth !== undefined
        ? draft.backdropMinWidth
        : (data.backdropMinWidth ?? null),
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
    writeImages: draft.writeImages ?? data.writeImages ?? false,
  };
  const languages = data.languages ?? [];

  const save = (patch: Partial<ScrapeDefaults>) => {
    const next = { ...value, ...patch };
    setDraft(next);
    void client.library.setScrapeDefaults(next).then(() => {
      setSaved(true);
      window.setTimeout(() => setSaved(false), 1600);
      void query.refetch();
    });
  };

  const poster: ArtRuleValue = {
    languages: value.posterLanguages ?? null,
    minWidth: value.posterMinWidth ?? null,
    size: value.posterSize ?? null,
  };
  const backdrop: ArtRuleValue = {
    languages: value.backdropLanguages ?? null,
    minWidth: value.backdropMinWidth ?? null,
    size: value.backdropSize ?? null,
  };
  const logo: ArtRuleValue = {
    languages: value.logoLanguages ?? null,
    minWidth: value.logoMinWidth ?? null,
    size: value.logoSize ?? null,
  };
  const season: ArtRuleValue = {
    languages: value.seasonLanguages ?? null,
    minWidth: value.seasonMinWidth ?? null,
    size: value.seasonSize ?? null,
  };
  const fanart = data.fanartConfigured === true;
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
      </PageHeader.Root>

      <Box mt={24} gap={20}>
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
            <Text variant="meta" color="success" minH={18}>
              {saved ? t('admin.scrapeSaved') : ''}
            </Text>
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
              sources={sourceProps(value.posterSources, ['tmdb', 'fanart'], 'posterSources')}
              onChange={(patch) =>
                save({
                  posterLanguages:
                    patch.languages !== undefined ? patch.languages : value.posterLanguages,
                  posterMinWidth:
                    patch.minWidth !== undefined ? patch.minWidth : value.posterMinWidth,
                  posterSize: patch.size !== undefined ? patch.size : value.posterSize,
                })
              }
            />
            <ArtRuleEditor
              title={t('admin.backdropPreference')}
              hint={t('admin.backdropRuleHint')}
              value={backdrop}
              languages={languages}
              minWidthPlaceholder="1920"
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
              minWidthPlaceholder="200"
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
              minWidthPlaceholder="500"
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
                disabled={!fanartKey.trim()}
                onPress={() => {
                  void client.library
                    .setScrapeDefaults({ fanartApiKey: fanartKey.trim() })
                    .then(() => {
                      setFanartKey('');
                      setFanartSaved(true);
                      window.setTimeout(() => setFanartSaved(false), 1600);
                      void query.refetch();
                    });
                }}
              />
              {fanart ? (
                <Button
                  variant="ghost"
                  size="sm"
                  label={t('admin.fanartClear')}
                  onPress={() => {
                    void client.library
                      .setScrapeDefaults({ clearFanartKey: true })
                      .then(() => void query.refetch());
                  }}
                />
              ) : null}
              {fanartSaved ? (
                <Text variant="meta" color="success">
                  {t('admin.scrapeSaved')}
                </Text>
              ) : null}
            </Row>
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
          </Box>
        </Surface>
      </Box>
    </>
  );
}