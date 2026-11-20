import { z } from 'zod';

/** The two browse namespaces Douban itself uses. */
export const DoubanKind = z.enum(['movie', 'tv']);
export type DoubanKind = z.infer<typeof DoubanKind>;

/** One Douban title as a poster draws it. `id` is Douban's subject id; it never
 * becomes a library id. */
export const DoubanCard = z.object({
  id: z.string(),
  kind: DoubanKind,
  title: z.string(),
  originalTitle: z.string(),
  year: z.number().nullish(),
  rating: z.number(),
  genres: z.array(z.string()),
  overview: z.string(),
  posterUrl: z.string(),
  backdropUrl: z.string().nullish(),
});
export type DoubanCard = z.infer<typeof DoubanCard>;

/** One row heading; the cards themselves are fetched per row. */
export const DoubanRowStub = z.object({
  id: z.string(),
  title: z.string(),
  ranked: z.boolean(),
});
export type DoubanRowStub = z.infer<typeof DoubanRowStub>;

/** `GET /api/douban/discover/{kind}`. Configuration only, no upstream call. */
export const DoubanLayout = z.object({ rows: z.array(DoubanRowStub) });
export type DoubanLayout = z.infer<typeof DoubanLayout>;

/** `GET /api/douban/rows/{kind}/{row}`. */
export const DoubanRow = z.object({
  id: z.string(),
  title: z.string(),
  ranked: z.boolean(),
  items: z.array(DoubanCard),
});
export type DoubanRow = z.infer<typeof DoubanRow>;

/** One row of Douban's own search results. */
export const DoubanSearchItem = z.object({
  id: z.string(),
  title: z.string(),
  rating: z.number(),
  posterUrl: z.string(),
});
export type DoubanSearchItem = z.infer<typeof DoubanSearchItem>;

export const DoubanPerson = z.object({
  name: z.string(),
  role: z.string().nullish(),
  avatarUrl: z.string().nullish(),
});
export type DoubanPerson = z.infer<typeof DoubanPerson>;

/** `GET /api/douban/subject/{id}`. */
export const DoubanDetail = z.object({
  card: DoubanCard,
  directors: z.array(DoubanPerson),
  cast: z.array(DoubanPerson),
  country: z.string(),
  language: z.string(),
  released: z.string(),
  aliases: z.array(z.string()),
  sourceUrl: z.string(),
});
export type DoubanDetail = z.infer<typeof DoubanDetail>;
