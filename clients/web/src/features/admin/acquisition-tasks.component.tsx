// What the server is fetching right now: the 115 cloud acquisitions the core
// ledger records, and the request queue (the BT path). Neither list is the whole
// picture on its own, which is why this tab reads both. Every row answers the
// same three questions — which title, where it is, what it cost.

import type { AcquisitionTask } from '@kromatv/client/acquisition';
import { useT } from '@kromatv/ui';
import { Badge, Button, Column, EmptyState, Row, Section, Text } from '@kromatv/ui/kit';
import { useNavigate } from '@tanstack/react-router';
import { usePoll } from '#web/features/admin/shell';
import { useAuth } from '#web/shared/lib/auth';
import { TableSkeleton } from '#web/shared/ui';

const ACTIVE = new Set(['approved', 'searching', 'downloading', 'importing', 'partially_available']);

type Tone = 'info' | 'warning' | 'danger' | 'success' | 'neutral';

const TONE: Record<string, Tone> = {
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
  candidate_ready: 'info',
  waiting_confirm: 'warning',
  transferring: 'info',
  verifying: 'info',
  done: 'success',
  succeeded: 'success',
};

/** The cloud attempt's status in the words a reader uses, not the ledger's. */
const CLOUD_STATE: Record<string, string> = {
  queued: 'queued',
  searching: 'searching',
  candidate_ready: 'ready',
  waiting_confirm: 'waitingConfirm',
  transferring: 'transferring',
  downloading: 'downloading',
  importing: 'importing',
  verifying: 'verifying',
  done: 'done',
  failed: 'failed',
};

/** The request queue's status in the same words. */
const REQUEST_STATE: Record<string, string> = {
  pending: 'pendingReview',
  approved: 'approved',
  searching: 'searching',
  downloading: 'downloading',
  importing: 'importing',
  partially_available: 'partial',
  available: 'done',
  denied: 'denied',
  failed: 'failed',
};

function when(ms: number): string {
  if (!ms) {
    return '';
  }
  const date = new Date(ms);
  return `${date.getMonth() + 1}-${String(date.getDate()).padStart(2, '0')} ${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`;
}

function CloudRow({ one }: Readonly<{ one: AcquisitionTask }>) {
  const t = useT();
  const key = CLOUD_STATE[one.status];
  const spent = one.costPoints && one.costPoints > 0 ? t('centre.cost', { points: one.costPoints }) : null;
  return (
    <Row gap={12} center p={12} radius="md" bg="surface1">
      <Column flex gap={2}>
        <Text variant="body" lines={1}>
          {one.title || one.unitKey}
        </Text>
        <Text variant="meta" color="textMuted" lines={1}>
          {[when(one.updatedAt || one.createdAt), spent].filter(Boolean).join(' · ')}
        </Text>
      </Column>
      {one.errorMessage ? (
        <Text variant="meta" color="danger" lines={1} style={{ maxWidth: 320 }}>
          {one.errorMessage}
        </Text>
      ) : null}
      <Badge tone={TONE[one.status] ?? 'neutral'}>{key ? t(`centre.state.${key}` as const) : one.status}</Badge>
    </Row>
  );
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
  // Cloud rows: everything recent, not only what is moving, so a finished or
  // failed grab still says what happened and what it cost.
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
    <Column gap={20}>
      {attempts.length > 0 ? (
        <Section.Root>
          <Section.Header>
            <Text variant="title">{t('centre.tasksCloud')}</Text>
          </Section.Header>
          <Column gap={2}>
            {attempts.map((one) => (
              <CloudRow key={one.id} one={one} />
            ))}
          </Column>
        </Section.Root>
      ) : null}
      {activeRequests.length > 0 ? (
        <Section.Root>
          <Section.Header>
            <Text variant="title">{t('centre.tasksRequests')}</Text>
          </Section.Header>
          <Column gap={2}>
            {activeRequests.map((one) => {
              const key = REQUEST_STATE[one.status];
              return (
                <Row key={one.id} gap={12} center p={12} radius="md" bg="surface1">
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
                  <Badge tone={TONE[one.status] ?? 'neutral'}>
                    {key ? t(`centre.state.${key}` as const) : one.status}
                  </Badge>
                  <Button
                    size="sm"
                    variant="ghost"
                    label={t('centre.taskOpen')}
                    onPress={() => void navigate({ to: '/admin/requests/$id', params: { id: one.id } })}
                  />
                </Row>
              );
            })}
          </Column>
        </Section.Root>
      ) : null}
    </Column>
  );
}