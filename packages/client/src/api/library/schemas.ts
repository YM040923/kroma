import { z } from 'zod';
import { LibraryId } from '../media';

/** Per-library scraping choices; null/empty follows the server default. */
export const ScrapeOptions = z.object({
  language: z.string().nullable(),
  nfo: z.boolean(),
  posterLanguage: z.string().nullable(),
  backdropLanguage: z.string().nullable(),
  /** Gallery backdrops kept per title; null keeps the provider's full set. */
  backdropCount: z.number().nullable().optional(),
  /** TMDB image size stored for provider URLs: w500, w780, w1280 or original. */
  artworkSize: z.string().nullable().optional(),
  /** Re-check show artwork every N days; null/0 disables the sweep. */
  artworkRefreshDays: z.number().nullable().optional(),
});
export type ScrapeOptions = z.infer<typeof ScrapeOptions>;

/** A named, multi-folder library (`GET /api/admin/libraries`). */
export const AdminLibrary = z.object({
  id: LibraryId,
  name: z.string(),
  kind: z.string(),
  folders: z.array(z.string()),
  itemCount: z.number(),
  sizeBytes: z.number(),
  lastScan: z.string().nullable(),
  autoScan: z.boolean(),
  scrape: ScrapeOptions,
  cover: z.string().nullish(),
  demo: z.boolean().default(false),
});
export type AdminLibrary = z.infer<typeof AdminLibrary>;

/** `POST /api/admin/libraries` body. */
export const LibraryBody = AdminLibrary.pick({ name: true, folders: true }).extend({
  kind: z.string().optional(),
});
export type LibraryBody = z.infer<typeof LibraryBody>;

/** `PATCH /api/admin/libraries/:id` body: only the keys present are changed. */
export const LibraryPatch = AdminLibrary.pick({
  name: true,
  kind: true,
  folders: true,
  autoScan: true,
  scrape: true,
}).exactPartial();
export type LibraryPatch = z.infer<typeof LibraryPatch>;
