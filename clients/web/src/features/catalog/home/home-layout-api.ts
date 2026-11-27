import type { WebRuntime } from '#web/shared/lib/api';
import type { HomeLayout } from './home-layout';

export async function saveAccountHomeLayout(runtime: WebRuntime, viewer: string, layout: HomeLayout): Promise<void> {
  const { queryKey } = runtime.queries.userQueries.homeLayout(viewer);
  const cancelRead = runtime.queryClient.cancelQueries({ queryKey, exact: true });
  // Dispatch under the current bearer before yielding to a profile switch.
  const [saved] = await Promise.all([runtime.client.accounts.setHomeLayout(layout), cancelRead]);
  runtime.queryClient.setQueryData(queryKey, saved);
}
