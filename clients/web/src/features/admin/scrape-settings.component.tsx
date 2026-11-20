import type { ScrapeDefaults } from '@kromatv/client/library';
import { useT } from '@kromatv/ui';
import { Box, Row, Surface, Switch, Text } from '@kromatv/ui/kit';
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