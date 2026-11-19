// Browser half of the explicit up-next gesture: a wheel / touch listener on the
// window (the player is the whole screen there) feeding the accumulator. A TV
// has no wheel and its swipe is already the D-pad's Down key, which opens the
// sheet from the control row; that platform uses the no-op twin of this file.

import { useEffect, useEffectEvent } from 'react';
import {
  createPeekGesture,
  type PeekGestureOptions,
} from '#ui/components/organisms/player/lib/peek-gesture';

export type { PeekGestureOptions } from '#ui/components/organisms/player/lib/peek-gesture';

export function usePeekGesture({ open, close, onOpen, onClose }: Readonly<PeekGestureOptions>): void {
  const openPeek = useEffectEvent(onOpen);
  const closePeek = useEffectEvent(onClose);

  useEffect(() => {
    if (!open && !close) {
      return undefined;
    }
    const gesture = createPeekGesture();
    // A touch is one gesture: its travel is measured from the finger-down
    // point, and a horizontal swipe belongs to the browser / stage, not here.
    let startY = 0;
    let startX = 0;
    let travelling = false;

    const onWheel = (event: WheelEvent) => {
      // ctrl + wheel is pinch zoom. It is the browser's gesture, not ours.
      if (event.ctrlKey) {
        return;
      }
      const step = gesture.feed(event.deltaY, event.timeStamp);
      if (step === 'down' && open) {
        openPeek();
      } else if (step === 'up' && close) {
        closePeek();
      }
    };

    const onTouchStart = (event: TouchEvent) => {
      const touch = event.touches[0];
      if (!touch) {
        travelling = false;
        return;
      }
      startX = touch.clientX;
      startY = touch.clientY;
      travelling = true;
      gesture.reset();
    };

    const onTouchMove = (event: TouchEvent) => {
      const touch = event.touches[0];
      if (!travelling || !touch) {
        return;
      }
      if (Math.abs(touch.clientX - startX) > Math.abs(touch.clientY - startY)) {
        return;
      }
      const step = gesture.feed(touch.clientY - startY, event.timeStamp);
      if (step === 'down' && open) {
        travelling = false;
        openPeek();
      } else if (step === 'up' && close) {
        travelling = false;
        closePeek();
      }
    };

    const onTouchEnd = () => {
      travelling = false;
      gesture.reset();
    };

    // Passive throughout: the player has nothing to scroll behind it, and the
    // open sheet's own list must keep the browser's wheel / touch scrolling.
    window.addEventListener('wheel', onWheel, { passive: true });
    window.addEventListener('touchstart', onTouchStart, { passive: true });
    window.addEventListener('touchmove', onTouchMove, { passive: true });
    window.addEventListener('touchend', onTouchEnd, { passive: true });
    window.addEventListener('touchcancel', onTouchEnd, { passive: true });
    return () => {
      window.removeEventListener('wheel', onWheel);
      window.removeEventListener('touchstart', onTouchStart);
      window.removeEventListener('touchmove', onTouchMove);
      window.removeEventListener('touchend', onTouchEnd);
      window.removeEventListener('touchcancel', onTouchEnd);
    };
  }, [open, close]);
}
