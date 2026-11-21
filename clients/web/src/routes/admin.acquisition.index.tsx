import { createFileRoute } from '@tanstack/react-router';
import { z } from 'zod';
import {
  AcquisitionCentrePage,
  type AcquisitionCentreTab,
} from '#web/features/admin/acquisition-centre.component';

const Search = z.object({
  tab: z.enum(['follow', 'subscriptions', 'tasks']).optional().catch(undefined),
});

export const Route = createFileRoute('/admin/acquisition/')({
  validateSearch: (raw: Record<string, unknown>) => Search.parse(raw),
  component: AcquisitionCentreRoute,
});

function AcquisitionCentreRoute() {
  const { tab = 'follow' } = Route.useSearch();
  const navigate = Route.useNavigate();
  return (
    <AcquisitionCentrePage
      tab={tab as AcquisitionCentreTab}
      onTab={(next) =>
        navigate({
          search: (prev) => ({ ...prev, tab: next === 'follow' ? undefined : next }),
          replace: true,
        })
      }
    />
  );
}