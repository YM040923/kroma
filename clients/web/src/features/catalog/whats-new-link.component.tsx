import { useT } from '@kromatv/ui';
import { Badge } from '@kromatv/ui/kit';
import { useQuery } from '@tanstack/react-query';
import { useWebRuntime } from '#web/shared/lib/web-runtime';

import { SideNav } from '#web/shared/ui/side-nav.component';

export function WhatsNewLink({ groupLabel }: Readonly<{ groupLabel?: string }> = {}) {
  const webRuntime = useWebRuntime();

  const t = useT();
  const { data } = useQuery({
    ...webRuntime.queries.userQueries.releases(),
    refetchOnWindowFocus: true,
  });
  if (!data?.unseen) {
    return null;
  }
  const link = (
    <SideNav.Item to="/whats-new" icon="sparkles">
      <SideNav.Label>{t('releases.title')}</SideNav.Label>
      <SideNav.Trailing>
        <Badge tone="warning">{data.current}</Badge>
      </SideNav.Trailing>
    </SideNav.Item>
  );
  return groupLabel ? <SideNav.Group label={groupLabel}>{link}</SideNav.Group> : link;
}
