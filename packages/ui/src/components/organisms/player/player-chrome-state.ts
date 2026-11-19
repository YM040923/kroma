import type { usePlayerNav } from './hooks/use-player-nav.hook';
import type { UpNextData } from './parts/up-next-sheet';

function initialSettingsView(overlay: string | null): 'audio' | 'subtitles' | 'menu' {
  if (overlay === 'audio') {
    return 'audio';
  }
  if (overlay === 'subtitles') {
    return 'subtitles';
  }
  return 'menu';
}

function hasUpNext(data: UpNextData): boolean {
  return data.nextEpisodes.length + data.recommendations.length > 0;
}

function deriveChrome(
  nav: ReturnType<typeof usePlayerNav>,
  upNext: UpNextData,
  panelCovers: boolean,
  over = false,
  /** The viewer asked for the peek with an explicit gesture (a wheel / swipe
   *  down), or this is a remote surface where it is always parked. Hovering no
   *  longer counts: the pointer roaming to the bottom of the stage used to pop
   *  the sheet on its own. */
  peekRequested = false,
  playing = false,
) {
  const settingsOpen =
    nav.overlay === 'settings' || nav.overlay === 'audio' || nav.overlay === 'subtitles';
  const sheetOpen = nav.overlay === 'sheet';
  const episodesOpen = nav.overlay === 'episodes';
  // Whether the picture is on the card, not HOW it got there: which hold the
  // card takes is the surface's business (see lib/surface-shrink).
  const settingsShrink = settingsOpen && !panelCovers;
  // The peek is a paused-time affordance: while the film is rolling it would
  // sit over the picture at the bottom, which is exactly the bar a viewer does
  // not want there.
  const peekVisible =
    !over &&
    nav.revealed &&
    hasUpNext(upNext) &&
    !settingsShrink &&
    !nav.overlay &&
    peekRequested &&
    !playing;
  // The episode drawer is a side panel with the transport still under it:
  // hiding the chrome would take the playback keys away for as long as the
  // viewer browses seasons, which on a phone is the whole screen.
  const chromeShown = !over && nav.revealed && (nav.overlay === null || episodesOpen);
  return { settingsOpen, sheetOpen, episodesOpen, settingsShrink, peekVisible, chromeShown };
}

export { deriveChrome, hasUpNext, initialSettingsView };
