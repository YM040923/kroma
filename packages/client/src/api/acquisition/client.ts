import type { RequestContext } from '../../core/client';
import {
  AcquisitionCandidates,
  type AcquisitionConfirmBody,
  AcquisitionConfirmResult,
  AcquisitionOutcome,
  type AcquisitionStartBody,
  AcquisitionStatusView,
} from './schemas';

/** Acquisition (`requests.create`): the one entry point a page uses to ask for
 *  a title, whichever provider ends up carrying it. A title the library already
 *  holds never leaves the server; a cloud-drive candidate is shown to the caller
 *  before its points are spent. */
export default function acquisitionApi(ctx: RequestContext) {
  return {
    /** The releases a search turned up, for the panel to list before anything is
     *  grabbed. Read-only: `start` or `confirm` still performs the grab. */
    candidates: (body: AcquisitionStartBody) =>
      ctx.post('/acquisition/candidates', AcquisitionCandidates, { body }),

    /** Ask for a title, or for some seasons/episodes of a show. The answer says
     *  what happened: owned, queued for review, started, needs a confirmation,
     *  or failed. With `candidate` set the named release is grabbed directly. */
    start: (body: AcquisitionStartBody) =>
      ctx.post('/acquisition/start', AcquisitionOutcome, { body }),

    /** Confirm the one 115 candidate a `needs_confirm` answer named, spending
     *  its points. Same ask, plus the candidate the caller was shown. */
    confirm: (body: AcquisitionConfirmBody) =>
      ctx.post('/acquisition/confirm', AcquisitionConfirmResult, { body }),

    /** How the attempt a `started`/`needs_confirm` answer named is getting on. */
    status: (attemptId: string) =>
      ctx.get('/acquisition/status', AcquisitionStatusView, { query: { attemptId } }),
  };
}

declare module '../../core/client' {
  interface Domains {
    acquisition: ReturnType<typeof acquisitionApi>;
  }
}
