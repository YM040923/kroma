import { useT } from '@kromatv/module-sdk';
import { PageHeader } from '@kromatv/ui/kit';
import { Cloud115Files } from './cloud115-page.component';

export default function Cloud115FilesPage() {
  const t = useT();
  return (
    <>
      <PageHeader.Root>
        <PageHeader.Title>{t('nav.cloud115Files')}</PageHeader.Title>
        <PageHeader.Subtitle>{t('cloud115.filesHint')}</PageHeader.Subtitle>
      </PageHeader.Root>
      <Cloud115Files />
    </>
  );
}
