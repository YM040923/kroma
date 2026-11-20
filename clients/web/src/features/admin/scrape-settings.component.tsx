import { useT } from '@kromatv/ui';
import { Box, Surface, Text } from '@kromatv/ui/kit';
import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';
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
  const [priority, setPriority] = useState<string[] | null>(null);
  const [saved, setSaved] = useState(false);

  if (query.isPending) {
    return null;
  }
  const value = priority ?? query.data?.languagePriority ?? [];
  const languages = query.data?.languages ?? [];

  const change = (next: string[]) => {
    if (next.length === 0) {
      return;
    }
    setPriority(next);
    void client.library.setScrapeDefaults(next).then(() => {
      setSaved(true);
      window.setTimeout(() => setSaved(false), 1600);
      void query.refetch();
    });
  };

  return (
    <>
      <PageHeader.Root>
        <PageHeader.Title>{t('admin.scrapeTitle')}</PageHeader.Title>
        <PageHeader.Subtitle>{t('admin.scrapeSub')}</PageHeader.Subtitle>
      </PageHeader.Root>

      <Box mt={24}>
        <Surface pad="none" radius="xl" border="border">
          <Box px={20} py={18} gap={12}>
            <Box>
              <Text variant="title">{t('admin.scrapeLanguageTitle')}</Text>
              <Text variant="meta" color="textDim" mt={4}>
                {t('admin.scrapeLanguageDesc')}
              </Text>
            </Box>
            <LanguagePriorityEditor
              value={value}
              languages={languages}
              min={1}
              onChange={change}
            />
            <Text variant="meta" color="success" minH={18}>
              {saved ? t('admin.scrapeSaved') : ''}
            </Text>
          </Box>
        </Surface>
      </Box>
    </>
  );
}