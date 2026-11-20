import { z } from 'zod';
import { LibraryId } from '../media';

/** Per-library scraping choices; null/empty follows the server default. */
export const ScrapeOptions = z.object({
  language: z.string().nullable(),
  /** Ordered languages: the first is asked for, the rest fill gaps. Null follows the household default. */
  languagePriority: z.array(z.string()).nullable().optional(),
  nfo: z.boolean(),
  posterLanguage: z.string().nullable(),
  backdropLanguage: z.string().nullable(),
  posterLanguages: z.array(z.string()).nullable().optional(),
  backdropLanguages: z.array(z.string()).nullable().optional(),
  posterMinWidth: z.number().nullable().optional(),
  backdropMinWidth: z.number().nullable().optional(),
  posterMinVotes: z.number().nullable().optional(),
  backdropMinVotes: z.number().nullable().optional(),
  posterSize: z.string().nullable().optional(),
  backdropSize: z.string().nullable().optional(),
  logoLanguages: z.array(z.string()).nullable().optional(),
  logoMinWidth: z.number().nullable().optional(),
  logoSize: z.string().nullable().optional(),
  seasonLanguages: z.array(z.string()).nullable().optional(),
  seasonMinWidth: z.number().nullable().optional(),
  seasonSize: z.string().nullable().optional(),
  posterSources: z.array(z.string()).nullable().optional(),
  backdropSources: z.array(z.string()).nullable().optional(),
  logoSources: z.array(z.string()).nullable().optional(),
  seasonSources: z.array(z.string()).nullable().optional(),
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

/** Household scraping defaults every library follows until it overrides them. */
export const ScrapeDefaults = z.object({
  languagePriority: z.array(z.string()),
  posterLanguages: z.array(z.string()).nullable().optional(),
  backdropLanguages: z.array(z.string()).nullable().optional(),
  posterMinWidth: z.number().nullable().optional(),
  backdropMinWidth: z.number().nullable().optional(),
  posterMinVotes: z.number().nullable().optional(),
  backdropMinVotes: z.number().nullable().optional(),
  posterSize: z.string().nullable().optional(),
  backdropSize: z.string().nullable().optional(),
  logoLanguages: z.array(z.string()).nullable().optional(),
  logoMinWidth: z.number().nullable().optional(),
  logoSize: z.string().nullable().optional(),
  seasonLanguages: z.array(z.string()).nullable().optional(),
  seasonMinWidth: z.number().nullable().optional(),
  seasonSize: z.string().nullable().optional(),
  posterSources: z.array(z.string()).nullable().optional(),
  backdropSources: z.array(z.string()).nullable().optional(),
  logoSources: z.array(z.string()).nullable().optional(),
  seasonSources: z.array(z.string()).nullable().optional(),
  writeImages: z.boolean().optional(),
});
export type ScrapeDefaults = z.infer<typeof ScrapeDefaults>;

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
