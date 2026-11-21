// What the server is fetching right now: the request queue seen as work, not as
// paperwork. Each row says where it is (queued, downloading, importing) and how
// far along, which is the question the acquisition centre exists to answer.

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
};

export function AcquisitionTasks() {
  const t = useT();
  const navigate = useNavigate();
  const { client } = useAuth();
  const { data, loading, failed } = usePoll(['admin', 'acquisition-tasks'], () => client.requests.list(), 8000);

  if (loading && !data) {
    return <TableSkeleton rows={4} />;
  }
  if (failed && !data) {
    return (
      <EmptyState.Root>
        <EmptyState.Title>{t('centre.tasksUnavailable')}</EmptyState.Title>
      </EmptyState.Root>
    );
  }
  const requests = data?.requests ?? [];
  const active = requests.filter((one) => ACTIVE.has(one.status));
  if (active.length === 0) {
    return (
      <EmptyState.Root>
        <EmptyState.Title>{t('centre.tasksEmpty')}</EmptyState.Title>
        <EmptyState.Hint>{t('centre.tasksEmptyHint')}</EmptyState.Hint>
      </EmptyState.Root>
    );
  }
  return (
    <Column gap={4}>
      {active.map((one) => (
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
  );
}