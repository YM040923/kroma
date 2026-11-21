// What the server is fetching right now, from both engines: the 115 cloud
// acquisitions the core ledger records, and the request queue (the BT path).
// Neither list is the whole picture on its own, which is why this tab reads
// both.

import { useT } from '@kromatv/ui';
import { Badge, Button, Column, EmptyState, Row, Text } from '@kromatv/ui/kit';
import { useNavigate } from '@tanstack/react-router';
import { usePoll } from '#web/features/admin/shell';
import { useAuth } from '#web/shared/lib/auth';
import { TableSkeleton } from '#web/shared/ui';

const ACTIVE = new Set(['approved', 'searching', 'downloading', 'importing', 'partially_available']);

const TONE: Record<string, 'info' | 'warning' | 'danger' | 'success' | 'neutral'> = {
  pending: 'warning',
  approved: 'info',
  searching: 'info',
  downloading: 'info',
  importing: 'info',
  partially_available: 'warning',
  available: 'success',
  failed: 'danger',
  denied: 'neutral',
  queued: 'warning',
  transferring: 'info',
  waiting_confirm: 'warning',
  done: 'success',
};

function when(ms: number): string {
  if (!ms) {
    return '';
  }
  const date = new Date(ms);
  return `${date.getMonth() + 1}-${String(date.getDate()).padStart(2, '0')} ${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`;
}

export function AcquisitionTasks() {
  const t = useT();
  const navigate = useNavigate();
  const { client } = useAuth();
  const requests = usePoll(['admin', 'acquisition-tasks'], () => client.requests.list(), 8000);
  const cloud = usePoll(['admin', 'acquisition-cloud-tasks'], () => client.acquisition.tasks(), 8000);

  if ((requests.loading && !requests.data) || (cloud.loading && !cloud.data)) {
    return <TableSkeleton rows={4} />;
  }

  const activeRequests = (requests.data?.requests ?? []).filter((one) => ACTIVE.has(one.status));
  const attempts = cloud.data?.attempts ?? [];

  if (activeRequests.length === 0 && attempts.length === 0) {
    if (requests.failed && cloud.failed) {
      return (
        <EmptyState.Root>
          <EmptyState.Title>{t('centre.tasksUnavailable')}</EmptyState.Title>
        </EmptyState.Root>
      );
    }
    return (
      <EmptyState.Root>
        <EmptyState.Title>{t('centre.tasksEmpty')}</EmptyState.Title>
        <EmptyState.Hint>{t('centre.tasksEmptyHint')}</EmptyState.Hint>
      </EmptyState.Root>
    );
  }

  return (
    <Column gap={16}>
      {attempts.length > 0 ? (
        <Column gap={4}>
          <Text variant="title">{t('centre.tasksCloud')}</Text>
          {attempts.map((one) => (
            <Row key={one.id} gap={12} center>
              <Column flex gap={2}>
                <Text variant="body">{one.unitKey.replace(/^[a-z]+:/, '')}</Text>
                <Text variant="meta" color="textMuted">
                  {[when(one.createdAt), one.costPoints ? t('centre.cost', { points: one.costPoints }) : null]
                    .filter(Boolean)
                    .join(' · ')}
                </Text>
              </Column>
              {one.errorMessage ? (
                <Text variant="meta" color="danger" lines={1}>
                  {one.errorMessage}
                </Text>
              ) : null}
              <Badge tone={TONE[one.status] ?? 'neutral'}>{one.status}</Badge>
            </Row>
          ))}
        </Column>
      ) : null}
      {activeRequests.length > 0 ? (
        <Column gap={4}>
          <Text variant="title">{t('centre.tasksRequests')}</Text>
          {activeRequests.map((one) => (
            <Row key={one.id} gap={12} center>
              <Column flex gap={2}>
                <Text variant="body">{one.title}</Text>
                <Text variant="meta" color="textMuted">
                  {[one.year ? String(one.year) : null, one.requestedByName ?? '']
                    .filter(Boolean)
                    .join(' · ')}
                </Text>
              </Column>
              {one.progress != null ? (
                <Text variant="meta" color="textMuted">
                  {Math.round(one.progress * 100)}%
                </Text>
              ) : null}
              <Badge tone={TONE[one.status] ?? 'neutral'}>{one.status}</Badge>
              <Button
                size="sm"
                variant="ghost"
                label={t('centre.taskOpen')}
                onPress={() => void navigate({ to: '/admin/requests/$id', params: { id: one.id } })}
              />
            </Row>
          ))}
        </Column>
      ) : null}
    </Column>
  );
}