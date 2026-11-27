import { useT } from '@kromatv/ui';
import { useQuery } from '@tanstack/react-query';
import { useModuleNav } from '#web/modules/module-host-provider.component';
import { useAuth } from '#web/shared/lib/auth';
import { useWebRuntime } from '#web/shared/lib/web-runtime';
import { sidebarInventory } from './sidebar-inventory';

export function useSidebarInventory() {
  const { user, ready } = useAuth();
  const runtime = useWebRuntime();
  const viewer = user?.id ?? '';
  const enabled = ready && !!user;
  const t = useT();
  const modules = useModuleNav('library');
  const layout = useQuery({ ...runtime.queries.userQueries.sidebarLayout(viewer), enabled });
  const libraries = useQuery({ ...runtime.queries.catalogQueries.libraries(viewer), enabled });
  const groups = sidebarInventory(t, enabled ? user : null, libraries.isSuccess ? libraries.data : [], modules);
  return { layout, libraries, groups };
}
