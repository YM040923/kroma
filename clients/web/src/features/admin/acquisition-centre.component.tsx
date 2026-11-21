// The acquisition centre: one admin page for what is being followed, what is
// being subscribed to, and what the server is fetching right now. The three
// tabs are the same three questions an operator asks after clicking "get".
//
// Monitoring and the request queue already have their own data; subscriptions
// live in a module, so this page reads that module's own admin routes rather
// than duplicating its storage.

import { useT } from '@kromatv/ui';
import { Box, Button, Column, EmptyState, Row, Section, Text } from '@kromatv/ui/kit';
import { PageHeader, useCap } from '#web/features/admin/shell';
import { MonitoredList } from '#web/features/admin/monitored-list.component';
import { AcquisitionTasks } from '#web/features/admin/acquisition-tasks.component';
import { SubscriptionGrid } from '#web/features/admin/subscription-grid.component';

export type AcquisitionCentreTab = 'follow' | 'subscriptions' | 'tasks';

const TABS: readonly AcquisitionCentreTab[] = ['follow', 'subscriptions', 'tasks'];

const LABEL = {
  follow: 'centre.tabFollow',
  subscriptions: 'centre.tabSubscriptions',
  tasks: 'centre.tabTasks',
} as const;

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
      <Row gap={8} center wrap mt={16} mb={16}>
        {TABS.map((one) => (
          <Button
            key={one}
            size="sm"
            variant={shown === one ? 'primary' : 'glass'}
            label={t(LABEL[one])}
            onPress={() => onTab(one)}
          />
        ))}
      </Row>
      {shown === 'follow' ? <FollowTab /> : null}
      {shown === 'subscriptions' ? (
        <SubscriptionGrid
          onOpen={() => {
            // A module page is not part of the core route tree, so it is opened
            // as a plain navigation rather than a typed route.
            window.location.assign('/admin/m/tv.kroma.subscriptions/subscriptions');
          }}
        />
      ) : null}
      {shown === 'tasks' ? <AcquisitionTasks /> : null}
    </>
  );
}

function FollowTab() {
  const t = useT();
  return (
    <Section.Root>
      <Section.Header>
        <Text variant="title">{t('centre.followTitle')}</Text>
      </Section.Header>
      <Column gap={8}>
        <Text variant="meta" color="textMuted">
          {t('centre.followHint')}
        </Text>
        <MonitoredList />
      </Column>
    </Section.Root>
  );
}

/** A page body wrapper the tabs share, so the spacing is one decision. */
export function CentreBody({ children }: Readonly<{ children: React.ReactNode }>) {
  return <Box mt={8}>{children}</Box>;
}