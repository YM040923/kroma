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
  /** The release the panel picked, echoed back as the search returned it.
   *  Absent means the server chooses. */
  candidate: z.record(z.string(), z.unknown()).nullish(),
  /** For a show: which season the panel lists. */
  season: z.number().nullish(),
});
export type AcquisitionStartBody = z.infer<typeof AcquisitionStartBody>;

/** What the library already holds of the title: `local` on disk, `cloud` on the
 *  drive it mirrors. Either is an answer: nothing is fetched, or requested. */
export const AcquisitionOwned = z.object({ local: z.boolean(), cloud: z.boolean() });
export type AcquisitionOwned = z.infer<typeof AcquisitionOwned>;

/** One 115 candidate a search turned up. `index` addresses it in that search,
 *  `size` may be unknown (a share listing omits it until it is probed), and
 *  `price` is what unlocking the share costs in points, when the provider says.
 *  It is handed back verbatim to `confirm`, which is why every field travels:
 *  the catch-all keeps the provider's own `id` and `extra` intact, and a schema
 *  that dropped them would hand the server a candidate it cannot identify. */
export const Cloud115Candidate = z
  .object({
    index: z.number(),
    fileName: z.string(),
    size: z.string().nullish(),
    sizeBytes: z.number().nullish(),
    price: z.number().nullish(),
    resource: z.string().nullish(),
    link: z.string().nullish(),
    shareCode: z.string().nullish(),
  })
  .catchall(z.unknown());
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
 *  caller was shown. `library` is not repeated, the attempt already has it; a
 *  show repeats the season its panel was listing. */
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

/** One row of the panel's list: the candidate, flattened, plus what the caller
 *  must do about it. The server lifts the provider's own fields (`index`,
 *  `fileName`, `link`, `shareCode`, `price`, `size`) out of `extra` to the top
 *  level, so a row reads as one object and goes back to `start`/`confirm` as
 *  one object. `recommended` marks the release the automatic path would take;
 *  `needsConfirm` is true when accepting costs points or the price is unknown. */
export const AcquisitionCandidate = z
  .object({
    id: z.string(),
    provider: z.string(),
    title: z.string(),
    index: z.number(),
    fileName: z.string().nullish(),
    size: z.string().nullish(),
    sizeBytes: z.number().nullish(),
    price: z.number().nullish(),
    pricePoints: z.number().nullish(),
    priceKnown: z.boolean().default(false),
    grabbable: z.boolean().default(true),
    resource: z.string().nullish(),
    link: z.string().nullish(),
    shareCode: z.string().nullish(),
    rejectReason: z.string().nullish(),
    needsConfirm: z.boolean(),
    recommended: z.boolean(),
  })
  .catchall(z.unknown());
export type AcquisitionCandidate = z.infer<typeof AcquisitionCandidate>;

/** The object handed back to `start`/`confirm` when a row is taken: the same
 *  flat object the list answered with, minus the two panel-only flags. */
export function candidatePayload(one: AcquisitionCandidate): Record<string, unknown> {
  const { needsConfirm: _needsConfirm, recommended: _recommended, ...fields } = one;
  return fields;
}

/** `POST /api/acquisition/candidates`: the list the acquisition panel draws.
 *  Nothing is charged or queued by asking; `blockers` says why the list is
 *  empty (provider missing, not signed in, the search failed), and `owned` is
 *  set when the title is already held so the panel can say so instead. */
export const AcquisitionCandidates = z.object({
  recommendedId: z.string().nullish(),
  candidates: z.array(AcquisitionCandidate),
  blockers: z.array(z.string()).default([]),
  owned: AcquisitionOwned.nullish(),
});
export type AcquisitionCandidates = z.infer<typeof AcquisitionCandidates>;

/** One cloud acquisition the server has run, as `GET /api/acquisition/tasks`
 *  answers it. A 115 grab has no request row, so this list is the only place the
 *  tasks tab can see it. */
export const AcquisitionTask = z.object({
  id: z.string(),
  unitKey: z.string(),
  provider: z.string(),
  status: z.string(),
  errorCode: z.string().nullish(),
  errorMessage: z.string().nullish(),
  jobId: z.string().nullish(),
  costPoints: z.number().nullish(),
  createdAt: z.number(),
  updatedAt: z.number(),
  finishedAt: z.number().nullish(),
});
export type AcquisitionTask = z.infer<typeof AcquisitionTask>;

/** `GET /api/acquisition/tasks`. */
export const AcquisitionTasks = z.object({
  attempts: z.array(AcquisitionTask).default([]),
});
export type AcquisitionTasks = z.infer<typeof AcquisitionTasks>;

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
