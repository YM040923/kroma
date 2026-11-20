import { createFileRoute } from '@tanstack/react-router';
import { ScrapeSettingsScreen } from '#web/features/admin/scrape-settings.component';

export const Route = createFileRoute('/admin/scrape')({
  component: ScrapeSettingsScreen,
});