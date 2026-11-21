import { z } from 'zod';
import { RequestId } from '../requests/ids';
import { EpisodeRef, RequestKind } from '../requests/schemas';

/** Who would carry an acquisition: the torrent engine, or the 115 cloud drive.
 *  The cloud drive is the only provider that can answer `needs_confirm`, because
 *  it is the only one that prices a candidate in points. */
export const AcquisitionProvider = z.enum(['bt', 'cloud115']);
export type AcquisitionProvider = z.infer<typeof AcquisitionProvider>;

/** `POST /api/acquisition/start` body. The title is named, not looked up: the
 *  page already knows which film or show it is, and the server must not guess.
 *  `seasons`/`episodes` narrow a show to what the caller picked; both null asks
 *  for the whole show. `library` names the library to aim at when a server
 *  holds several. */
export const AcquisitionStartBody = z.object({
  kind: RequestKind,
  tmdbId: z.number(),
  title: z.string(),
  year: z.number().nullish(),
  seasons: z.array(z.number()).nullish(),
  episodes: z.array(EpisodeRef).nullish(),
  library: z.string().nullish(),
});
export type AcquisitionStartBody = z.infer<typeof AcquisitionStartBody>;

/** What the library already holds of the title: `local` on disk, `cloud` on the
 *  drive it mirrors. Either is an answer: nothing is fetched, or requested. */
export const AcquisitionOwned = z.object({ local: z.boolean(), cloud: z.boolean() });
export type AcquisitionOwned = z.infer<typeof AcquisitionOwned>;

/** One 115 candidate a search turned up. `index` addresses it in that search,
 *  `size` may be unknown (a share listing omits it until it is probed), and
 *  `price` is what unlocking the share costs in points, when the provider says.
 *  It is handed back verbatim to `confirm`, which is why every field travels. */
export const Cloud115Candidate = z.object({
  index: z.number(),
  fileName: z.string(),
  size: z.string().nullish(),
  sizeBytes: z.number().nullish(),
  price: z.number().nullish(),
  resource: z.string().nullish(),
  link: z.string().nullish(),
  shareCode: z.string().nullish(),
});
export type Cloud115Candidate = z.infer<typeof Cloud115Candidate>;

/** What confirming would cost. `priceKnown` is the flag that matters: false
 *  means the provider did not price it, so `points` must not be read as free. */
export const Cloud115Quote = z.object({
  points: z.number().nullable(),
  priceKnown: z.boolean(),
});
export type Cloud115Quote = z.infer<typeof Cloud115Quote>;

/** The state a started acquisition answers with, for both `start` and
 *  `confirm`: which provider took it, and whichever handles the provider mints
 *  (a request, an attempt, a job) for following it afterwards. */
export const AcquisitionStarted = z.object({
  state: z.literal('started'),
  provider: AcquisitionProvider,
  requestId: RequestId.nullish(),
  attemptId: z.string().nullish(),
  jobId: z.string().nullish(),
});
export type AcquisitionStarted = z.infer<typeof AcquisitionStarted>;

/** `POST /api/acquisition/start`. One of: the library already has it, a review
 *  queue took it, an acquisition started, the caller must pick and confirm a
 *  115 candidate, or nothing happened and `error` says why. */
export const AcquisitionOutcome = z.discriminatedUnion('state', [
  z.object({ state: z.literal('owned'), owned: AcquisitionOwned }),
  z.object({ state: z.literal('pending_review'), requestId: RequestId }),
  AcquisitionStarted,
  z.object({
    state: z.literal('needs_confirm'),
    provider: z.literal('cloud115'),
    candidate: Cloud115Candidate,
    quote: Cloud115Quote,
    attemptId: z.string(),
  }),
  z.object({ state: z.literal('failed'), error: z.string() }),
]);
export type AcquisitionOutcome = z.infer<typeof AcquisitionOutcome>;

/** `POST /api/acquisition/confirm` body: the same ask, plus the candidate the
 *  caller was shown. `library` is not repeated, the attempt already has it. */
export const AcquisitionConfirmBody = AcquisitionStartBody.omit({ library: true }).extend({
  candidate: Cloud115Candidate,
});
export type AcquisitionConfirmBody = z.infer<typeof AcquisitionConfirmBody>;

/** `POST /api/acquisition/confirm`: a started acquisition, or what stopped it. */
export const AcquisitionConfirmResult = z.discriminatedUnion('state', [
  AcquisitionStarted,
  z.object({ state: z.literal('failed'), error: z.string() }),
]);
export type AcquisitionConfirmResult = z.infer<typeof AcquisitionConfirmResult>;

/** `GET /api/acquisition/status?attemptId=`. The attempt as its provider
 *  reports it: `state` is the provider's own word (`queued`, `running`,
 *  `done`, `failed`, ...), and everything else is what the provider could say
 *  about it. The fields past the state are optional so a provider that reports
 *  less still parses. */
export const AcquisitionStatusView = z.object({
  attemptId: z.string().nullish(),
  state: z.string(),
  progress: z.number().nullish(),
  message: z.string().nullish(),
  error: z.string().nullish(),
  requestId: RequestId.nullish(),
});
export type AcquisitionStatusView = z.infer<typeof AcquisitionStatusView>;
