// The up-next peek used to open on hover: the pointer reaching the bottom of
// the stage popped the sheet on its own, which on a paused film reads as a
// misfire rather than a request. Only an explicit downward gesture opens it
// now, and an upward one closes it again.
//
// A wheel is delivered as a stream of small deltas, a swipe as one large jump:
// both feed the same accumulator, so a trackpad nudge never fires on its first
// event, a mouse wheel's notched 100 px does, and a direction change starts
// over instead of cancelling out.

/** How far the pointer / finger has to travel downward before the peek opens. */
export const PEEK_GESTURE_THRESHOLD = 72;
/** A pause in the stream starts a new gesture, so two unrelated nudges a second
 *  apart never add up to one that was never made. */
export const PEEK_GESTURE_IDLE_MS = 260;

export type PeekGestureStep = 'down' | 'up' | null;

/** What the platform hook listens for. `open` / `close` gate each direction
 *  independently: while the sheet is open a downward wheel belongs to its list,
 *  but an upward one still puts the peek away. */
export interface PeekGestureOptions {
  open: boolean;
  close: boolean;
  onOpen: () => void;
  onClose: () => void;
}

export interface PeekGesture {
  /** Feed one vertical delta (px, positive = downward) and the moment it was
   *  seen (ms). Answers once per completed gesture. */
  feed: (delta: number, at: number) => PeekGestureStep;
  reset: () => void;
}

export function createPeekGesture(
  threshold = PEEK_GESTURE_THRESHOLD,
  idleMs = PEEK_GESTURE_IDLE_MS,
): PeekGesture {
  let travelled = 0;
  let lastSign = 0;
  let lastAt = 0;
  const reset = () => {
    travelled = 0;
    lastSign = 0;
  };
  return {
    feed(delta, at) {
      if (!Number.isFinite(delta) || !Number.isFinite(at) || delta === 0) {
        return null;
      }
      if (lastAt !== 0 && at - lastAt > idleMs) {
        reset();
      }
      const sign = delta > 0 ? 1 : -1;
      if (sign !== lastSign) {
        travelled = 0;
        lastSign = sign;
      }
      lastAt = at;
      travelled += delta;
      if (travelled >= threshold) {
        reset();
        return 'down';
      }
      if (travelled <= -threshold) {
        reset();
        return 'up';
      }
      return null;
    },
    reset,
  };
}
