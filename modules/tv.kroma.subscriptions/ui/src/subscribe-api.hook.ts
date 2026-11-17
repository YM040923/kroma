import { moduleApiHook } from '@kromatv/module-sdk';
import { z } from 'zod';

export const Candidate = z.object({
  index: z.number(),
  fileName: z.string(),
  size: z.string().default(''),
  price: z.number().default(0),
  tags: z.array(z.string()).default([]),
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
  price: number;
}

export const useSubscribeApi = moduleApiHook((api) => ({
  search: (body: SearchInput) => api.post('/search', body, SearchView),
  subscribe: (body: SubscribeInput) => api.post('/subscribe', body, TaskView),
}));
