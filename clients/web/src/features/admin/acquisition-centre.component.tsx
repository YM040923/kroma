// The acquisition centre: one admin page for three questions a viewer asks
// after pressing "get" — what am I following, what am I subscribed to, and what
// is the server doing right now. Everything the old Subscriptions module page
// did lives here too, so there is one place, not two.

import type { MessageKey } from '@kromatv/i18n';
import { useT } from '@kromatv/ui';
import { Box, EmptyState, SegmentGroup } from '@kromatv/ui/kit';
import { PageHeader, useCap } from '#web/features/admin/shell';
import { AcquisitionTasks } from '#web/features/admin/acquisition-tasks.component';
import { FollowList } from '#web/features/admin/follow-list.component';
import { SubscriptionGrid } from '#web/features/admin/subscription-grid.component';

export type AcquisitionCentreTab = 'follow' | 'subscriptions' | 'tasks';

const TABS: readonly AcquisitionCentreTab[] = ['follow', 'subscriptions', 'tasks'];

const LABEL: Record<AcquisitionCentreTab, MessageKey> = {
  follow: 'centre.tabFollow',
  subscriptions: 'centre.tabSubscriptions',
  tasks: 'centre.tabTasks',
};

export function AcquisitionCentrePage({
  tab,
  onTab,
}: Readonly<{ tab: AcquisitionCentreTab; onTab: (tab: AcquisitionCentreTab) => void }>) {
  const t = useT();
  const canManage = useCap('requests.manage');
  if (!canManage) {
    return <EmptyState.Root icon="lock">{t('centre.needManage')}</EmptyState.Root>;
  }
  const shown = TABS.includes(tab) ? tab : 'follow';
  return (
    <>
      <PageHeader.Root>
        <PageHeader.Title>{t('centre.title')}</PageHeader.Title>
        <PageHeader.Subtitle>{t('centre.subtitle')}</PageHeader.Subtitle>
      </PageHeader.Root>
      <Box mt={20} mb={20} align="flex-start">
        <SegmentGroup.Root value={shown} onValueChange={(next) => onTab(next)} label={t('centre.title')}>
          {TABS.map((one) => (
            <SegmentGroup.Item key={one} value={one}>
              <SegmentGroup.Label>{t(LABEL[one])}</SegmentGroup.Label>
            </SegmentGroup.Item>
          ))}
        </SegmentGroup.Root>
      </Box>
      {shown === 'follow' ? <FollowList /> : null}
      {shown === 'subscriptions' ? <SubscriptionGrid /> : null}
      {shown === 'tasks' ? <AcquisitionTasks /> : null}
    </>
  );
}