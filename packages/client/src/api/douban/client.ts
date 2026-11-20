import type { RequestContext } from '../../core/client';
import {
  DoubanDetail,
  type DoubanKind,
  DoubanLayout,
  DoubanRow,
  DoubanSearchItem,
} from './schemas';

/** Douban's browse rows and one subject's detail. Read-only: nothing here
 * links a Douban id to the library. */
export default function doubanApi(ctx: RequestContext) {
  return {
    /** The rows this source offers for a kind; cards are fetched per row. */
    layout: (kind: DoubanKind) =>
      ctx.get('/douban/discover/:kind', DoubanLayout, {
        params: { kind },
        concurrency: 'share',
      }),

    /** One row's cards. */
    row: (kind: DoubanKind, row: string) =>
      ctx.get('/douban/rows/:kind/:row', DoubanRow, {
        params: { kind, row },
        concurrency: 'share',
      }),

    /** Douban's own search results for a text query. */
    search: (q: string) =>
      ctx.get('/douban/search', DoubanSearchItem.array(), {
        query: { q },
        concurrency: 'latest',
      }),

    /** One subject's detail (rating, synopsis, cast). */
    subject: (id: string) =>
      ctx.get('/douban/subject/:id', DoubanDetail, {
        params: { id },
        concurrency: 'share',
      }),

    /** Douban's CDN rejects a fetch without its own referer, so posters load
     * through this server-side hop rather than straight from the browser. */
    imageUrl: (url: string | null | undefined): string | null =>
      url ? `/api/douban/image?url=${encodeURIComponent(url)}` : null,
  };
}

declare module '../../core/client' {
  interface Domains {
    douban: ReturnType<typeof doubanApi>;
  }
}
