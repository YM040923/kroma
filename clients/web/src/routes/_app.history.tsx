import { createFileRoute } from '@tanstack/react-router';
import { HistoryScreen } from '#web/features/catalog/history-screen.component';

export const Route = createFileRoute('/_app/history')({
  component: HistoryScreen,
});
