// The one acquisition flow every surface shares: the title page's Acquire
// button, its season and request-bar actions, and the discover card. A 115
// candidate is confirmed here, once, so no caller has to know the provider.

import { apiErrorText, type KromaClient } from '@kromatv/client';
import type {
  AcquisitionOutcome,
  AcquisitionProvider,
  AcquisitionStartBody,
} from '@kromatv/client/acquisition';
import type { EpisodeRef } from '@kromatv/client/requests';
import type { Translate } from '@kromatv/i18n';
import { useFormat, useT } from '@kromatv/ui';
import { useConfirm } from '@kromatv/ui/kit';
import { useCallback } from 'react';
import { useAuth } from '#web/shared/lib/auth';

/** The title an acquisition aims at: resolved before the ask, never guessed. */
export interface AcquisitionTarget {
  kind: 'movie' | 'show';
  tmdbId: number;
  title: string;
  year: number | null;
}

/** What part of a show to ask for. Both null asks for the whole show. */
export interface AcquisitionScope {
  seasons?: number[] | null;
  episodes?: EpisodeRef[] | null;
}

/** What became of an ask. `cancelled` is the caller's own answer to a
 *  confirmation, so it is not an error and patches nothing. */
export type AcquisitionResult =
  | { state: 'owned'; owned: { local: boolean; cloud: boolean } }
  | { state: 'pending_review' }
  | { state: 'started'; provider: AcquisitionProvider }
  | { state: 'cancelled' }
  | { state: 'failed'; error: string };

/** An optional release the caller already picked in the panel. Present, the
 *  server grabs exactly that one; absent, it chooses. `season` names what a
 *  show's panel was listing. */
export interface AcquirePick {
  candidate?: Record<string, unknown> | null;
  season?: number | null;
}

export type Acquire = (
  target: AcquisitionTarget,
  scope?: AcquisitionScope,
  pick?: AcquirePick,
) => Promise<AcquisitionResult>;

type NeedsConfirm = Extract<AcquisitionOutcome, { state: 'needs_confirm' }>;
type Ask = ReturnType<typeof useConfirm>;

/** "Release: … Size: … Cost: …", with the points said to be unknown when the
 *  provider did not price the candidate: an unpriced release must never read as
 *  a free one. */
function confirmMessage(t: Translate, bytes: (n: number) => string, offer: NeedsConfirm): string {
  const { candidate, quote } = offer;
  const name = candidate.fileName || candidate.resource || candidate.shareCode || `#${candidate.index}`;
  const size =
    candidate.sizeBytes != null && candidate.sizeBytes > 0
      ? bytes(candidate.sizeBytes)
      : candidate.size || t('requests.acquireUnknownSize');
  const cost =
    quote.priceKnown && quote.points != null
      ? t('requests.acquireCost', { points: quote.points })
      : t('requests.acquireCostUnknown');
  return [
    t('requests.acquireCandidate', { file: name }),
    t('requests.acquireCandidateSize', { size }),
    cost,
    t('requests.acquireConfirmNote'),
  ].join(' ');
}

/** The ask, as the endpoint wants it: the whole title unless a scope narrows
 *  it to some seasons or episodes of a show. */
function askBody(target: AcquisitionTarget, scope: AcquisitionScope): AcquisitionStartBody {
  return {
    kind: target.kind,
    tmdbId: target.tmdbId,
    title: target.title,
    year: target.year,
    seasons: scope.seasons ?? null,
    episodes: scope.episodes ?? null,
    // A show's confirm repeats the season its panel was listing.
    season: target.kind === 'show' ? (scope.seasons?.[0] ?? null) : null,
  };
}

/** The outcome of `/start` as this hook reports it: a started acquisition names
 *  its provider, the rest pass through unchanged. */
function toResult(outcome: AcquisitionOutcome): AcquisitionResult {
  switch (outcome.state) {
    case 'owned':
      return { state: 'owned', owned: outcome.owned };
    case 'pending_review':
      return { state: 'pending_review' };
    case 'started':
      return { state: 'started', provider: outcome.provider };
    case 'failed':
      return { state: 'failed', error: outcome.error };
    case 'needs_confirm':
      // A caller that reached here without confirming asked for a report, not a
      // charge: treat it as "nothing was taken".
      return { state: 'cancelled' };
  }
}

/** What confirming needs: the transport, how to speak, and how to ask. */
interface ConfirmDeps {
  ask: Ask;
  client: KromaClient;
  t: Translate;
  bytes: (n: number) => string;
}

/** The confirm half of an ask: name the candidate and its price, and only then
 *  spend the points. A refused or dismissed question asks for nothing. */
async function confirmCandidate(
  deps: ConfirmDeps,
  body: AcquisitionStartBody,
  offer: NeedsConfirm,
): Promise<AcquisitionResult> {
  const { ask, client, t } = deps;
  const ok = await ask({
    title: t('requests.acquireConfirmTitle'),
    message: confirmMessage(t, deps.bytes, offer),
    confirmLabel: t('requests.acquireConfirm'),
    cancelLabel: t('common.cancel'),
  });
  if (!ok) {
    return { state: 'cancelled' };
  }
  const done = await client.acquisition.confirm({ ...body, candidate: offer.candidate });
  return done.state === 'started'
    ? { state: 'started', provider: done.provider }
    : { state: 'failed', error: done.error };
}

/** Ask for a title, confirm a priced candidate when one must be picked, and say
 *  what happened. With `pick.candidate` the caller already chose in the panel:
 *  the server grabs exactly that release, and the only thing left is the
 *  confirmation when it costs points. A transport failure comes back as
 *  `failed` rather than throwing, so every caller patches its view the same
 *  way. */
export function useAcquisition(): Acquire {
  const t = useT();
  const fmt = useFormat();
  const confirm = useConfirm();
  const { client } = useAuth();

  return useCallback<Acquire>(
    async (target, scope = {}, pick = {}) => {
      const body = askBody(target, scope);
      // The panel's pick travels on the same `start` call: the server then does
      // exactly what `/confirm` would, so the price is the one quoted for this
      // release.
      const picked = pick.candidate ?? null;
      try {
        if (picked) {
          const done = await client.acquisition.start({
            ...body,
            candidate: picked,
            season: pick.season ?? null,
          });
          if (done.state === 'needs_confirm') {
            return await confirmCandidate({ ask: confirm, client, t, bytes: fmt.bytes }, body, done);
          }
          return toResult(done);
        }
        const outcome = await client.acquisition.start(body);
        if (outcome.state !== 'needs_confirm') {
          return outcome;
        }
        return await confirmCandidate({ ask: confirm, client, t, bytes: fmt.bytes }, body, outcome);
      } catch (error) {
        return { state: 'failed', error: apiErrorText(error, t('discover.requestFailed')) };
      }
    },
    [client, confirm, fmt, t],
  );
}
