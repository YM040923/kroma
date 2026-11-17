import { moduleApiHook } from '@kromatv/module-sdk';
import { z } from 'zod';

export const Candidate = z.object({
  index: z.number(),
  fileName: z.string().default(''),
  size: z.string().default(''),
  price: z.number().nullable().default(null),
  tags: z.array(z.string()).default([]),
  sizeBytes: z.number().default(0),
  unlockCount: z.number().default(0),
  sharer: z.string().default(''),
  date: z.string().default(''),
  link: z.string().default(''),
  shareCode: z.string().default(''),
  alreadyUnlocked: z.boolean().default(false),
  probeError: z.string().nullable().default(null),
});
export type SubscriptionCandidate = z.infer<typeof Candidate>;

const SearchView = z.object({ candidates: z.array(Candidate).default([]) });
const TaskView = z.object({ id: z.string(), status: z.string() }).passthrough();

export interface SearchInput {
  kind: 'movie' | 'show';
  tmdbId: number;
  title: string;
}

export interface SubscribeInput {
  kind: 'movie' | 'show';
  tmdbId: number;
  title: string;
  resource: string;
  index: number;
  price: number;
  poster: string;
  year: number;
  link: string;
}

export const useSubscribeApi = moduleApiHook((api) => ({
  search: (body: SearchInput) => api.post('/search', body, SearchView),
  subscribe: (body: SubscribeInput) => api.post('/subscribe', body, TaskView),
}));
