import { z } from 'zod';

/** `GET/POST /api/items/:id/share` — the live link an admin made for a title.
 * `password` is only present in the answer that created it. */
export const ShareView = z.object({
  slug: z.string(),
  path: z.string(),
  expiresAt: z.number(),
  requiresPassword: z.boolean(),
  viewCount: z.number().default(0),
  password: z.string().nullish(),
});
export type ShareView = z.infer<typeof ShareView>;

/** `GET /api/share/:slug` — what a visitor may know. Before the code, only the
 * fact that one is needed. */
export const ShareInfo = z.object({
  requiresPassword: z.boolean(),
  unlocked: z.boolean(),
  expiresAt: z.number(),
  title: z.string().nullish(),
  year: z.number().nullish(),
  overview: z.string().nullish(),
  posterUrl: z.string().nullish(),
  kind: z.string().nullish(),
  hasFile: z.boolean().nullish(),
});
export type ShareInfo = z.infer<typeof ShareInfo>;

export const ShareUnlocked = z.object({ unlocked: z.boolean() });
export type ShareUnlocked = z.infer<typeof ShareUnlocked>;
