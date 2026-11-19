// Native half: a television's swipe arrives as the D-pad's Down key and its
// Down already opens the up-next sheet from the control row (see
// `usePlayerNav.chromeKey`). There is no wheel to read, so this is a no-op that
// keeps the call site platform-free.

import type { PeekGestureOptions } from '#ui/components/organisms/player/lib/peek-gesture';

export type { PeekGestureOptions } from '#ui/components/organisms/player/lib/peek-gesture';

export function usePeekGesture(_options: Readonly<PeekGestureOptions>): void {
  // Nothing to listen to: the remote is the gesture.
}
