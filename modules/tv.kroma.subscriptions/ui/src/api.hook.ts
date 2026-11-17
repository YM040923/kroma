import { moduleApiHook } from '@kromatv/module-sdk';
import { z } from 'zod';

const Task = z.object({ id: z.string(), name: z.string(), source: z.string(), monitorPath: z.string(), targetPath: z.string(), failurePath: z.string(), monitorMode: z.string(), enabled: z.boolean(), status: z.string(), lastRunAt: z.number(), lastError: z.string(), tmdbId: z.number().default(0), resource: z.string().default(''), price: z.number().default(0) });
const Item = z.object({ key: z.string(), taskId: z.string(), title: z.string(), state: z.string(), lastError: z.string() });
const View = z.object({ sourceUrl: z.string().default(''), tasks: z.array(Task), items: z.array(Item) });
export type SubscriptionTask = z.infer<typeof Task>;
export type SubscriptionItem = z.infer<typeof Item>;
export interface SubscriptionTaskInput {
  name: string;
  source: string;
  monitorPath: string;
  targetPath: string;
  failurePath: string;
  monitorMode: string;
  enabled: boolean;
}
export const useSubscriptionsApi = moduleApiHook((api) => ({
  list: () => api.get('/subscriptions', View),
  create: (body: SubscriptionTaskInput) => api.post('/subscriptions', body, Task),
  update: (id: string, body: SubscriptionTaskInput) => api.put(`/subscriptions/${id}`, body, Task),
  scan: (id: string) => api.post(`/subscriptions/${id}/scan`, {}, z.array(Item)),
  source: (url: string) => api.post('/source', { url }, View),
}));
