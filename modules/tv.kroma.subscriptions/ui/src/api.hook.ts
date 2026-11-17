import { moduleApiHook } from '@kromatv/module-sdk';
import { z } from 'zod';

const Task = z.object({ id: z.string(), name: z.string(), source: z.string(), monitorPath: z.string(), targetPath: z.string(), failurePath: z.string(), monitorMode: z.string(), enabled: z.boolean(), status: z.string(), lastRunAt: z.number(), lastError: z.string() });
const Item = z.object({ key: z.string(), taskId: z.string(), title: z.string(), state: z.string(), lastError: z.string() });
const View = z.object({ tasks: z.array(Task), items: z.array(Item) });
export type SubscriptionTask = z.infer<typeof Task>;
export type SubscriptionItem = z.infer<typeof Item>;
export const useSubscriptionsApi = moduleApiHook((api) => ({
  list: () => api.get('/subscriptions', View),
  create: (body: Omit<SubscriptionTask, 'id' | 'status' | 'lastRunAt' | 'lastError'>) => api.post('/subscriptions', body, Task),
  update: (id: string, body: Omit<SubscriptionTask, 'id' | 'status' | 'lastRunAt' | 'lastError'>) => api.put(`/subscriptions/${id}`, body, Task),
  scan: (id: string) => api.post(`/subscriptions/${id}/scan`, {}, z.array(Item)),
}));
