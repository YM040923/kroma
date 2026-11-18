import { moduleApiHook } from '@kromatv/module-sdk';
import { z } from 'zod';

const Task = z.object({
  id: z.string(),
  name: z.string(),
  source: z.string(),
  monitorPath: z.string(),
  targetPath: z.string(),
  failurePath: z.string(),
  monitorMode: z.string(),
  enabled: z.boolean(),
  status: z.string(),
  lastRunAt: z.number(),
  lastError: z.string(),
  tmdbId: z.number().default(0),
  kind: z.string().default(''),
  resource: z.string().default(''),
  resourceIndex: z.number().default(0),
  price: z.number().default(0),
  jobId: z.string().default(''),
  poster: z.string().default(''),
  year: z.number().default(0),
  season: z.number().default(0),
  episodeStart: z.number().default(0),
  episodeTotal: z.number().default(0),
  link: z.string().default(''),
});
const Item = z.object({
  key: z.string(),
  taskId: z.string(),
  title: z.string(),
  state: z.string(),
  lastError: z.string(),
  resource: z.string().default(''),
  index: z.number().default(0),
  size: z.string().default(''),
  link: z.string().default(''),
  price: z.number().default(0),
});
const View = z.object({ sourceUrl: z.string().default(''), tasks: z.array(Task), items: z.array(Item) });
const CalendarEntry = z.object({
  date: z.string(),
  title: z.string(),
  season: z.number().default(0),
  episode: z.number().default(0),
  episodeName: z.string().default(''),
  poster: z.string().default(''),
  taskId: z.string().default(''),
});
const CalendarView = z.object({
  entries: z.array(CalendarEntry),
  updatedAt: z.number().default(0),
});
const Preference = z.object({
  minSizeGb: z.number().default(0),
  maxSizeGb: z.number().default(0),
  preferSub: z.boolean().default(true),
  penalizeOffline: z.boolean().default(true),
  preferMkv: z.boolean().default(true),
  maxPoints: z.number().default(0),
});
const SourceConfig = z.object({
  targetCid: z.string().default(''),
  preference: Preference,
  probeLimit: z.number().default(8),
});
export type SubscriptionTask = z.infer<typeof Task>;
export type SubscriptionItem = z.infer<typeof Item>;
export type SubscriptionCalendarEntry = z.infer<typeof CalendarEntry>;
export type SubscriptionPreference = z.infer<typeof Preference>;
export type SubscriptionConfig = z.infer<typeof SourceConfig>;
export interface SubscriptionConfigInput {
  targetCid?: string;
  probeLimit?: number;
  preference?: Partial<SubscriptionPreference>;
}
export interface SubscriptionTaskInput {
  name: string;
  source: string;
  monitorPath: string;
  targetPath: string;
  failurePath: string;
  monitorMode: string;
  enabled: boolean;
  poster: string;
  year: number;
  season: number;
  episodeStart: number;
  episodeTotal: number;
  link: string;
  tmdbId: number;
  kind: string;
}
export const useSubscriptionsApi = moduleApiHook((api) => ({
  list: () => api.get('/subscriptions', View),
  create: (body: SubscriptionTaskInput) => api.post('/subscriptions', body, Task),
  update: (id: string, body: SubscriptionTaskInput) => api.put(`/subscriptions/${id}`, body, Task),
  scan: (id: string) => api.post(`/subscriptions/${id}/scan`, {}, z.array(Item)),
  execute: (id: string) => api.post(`/subscriptions/${id}/execute`, {}, Task),
  executeItem: (id: string, body: { resource: string; index: number; price: number; link: string }) =>
    api.post(`/subscriptions/${id}/execute-item`, body, Task),
  source: (url: string) => api.post('/source', { url }, View),
  config: () => api.get('/config', SourceConfig),
  saveConfig: (body: SubscriptionConfigInput) => api.post('/config', body, SourceConfig),
  calendar: () => api.get('/calendar', CalendarView),
}));
