import type { RequestContext } from '../../core/client';
import type { ItemId } from '../media';
import { ShareInfo, ShareUnlocked, ShareView } from './schemas';

export interface ShareOptions {
  days?: number;
  password?: string | null;
}

/** Public links to one title, and the admin side that mints them. */
export default function shareApi(ctx: RequestContext) {
  return {
    current: (id: ItemId) =>
      ctx.get('/items/:id/share', ShareView.nullable(), { params: { id } }),

    create: (id: ItemId, options?: ShareOptions) =>
      ctx.post('/items/:id/share', ShareView, { params: { id }, body: options ?? {} }),

    revoke: (id: ItemId) => ctx.delete('/items/:id/share', { params: { id } }),

    info: (slug: string) =>
      ctx.get('/share/:slug', ShareInfo, { params: { slug }, concurrency: 'share' }),

    unlock: (slug: string, password: string) =>
      ctx.post('/share/:slug/unlock', ShareUnlocked, { params: { slug }, body: { password } }),

    streamUrl: (slug: string) => ctx.url('/share/:slug/stream', { params: { slug } }),
  };
}

declare module '../../core/client' {
  interface Domains {
    share: ReturnType<typeof shareApi>;
  }
}
