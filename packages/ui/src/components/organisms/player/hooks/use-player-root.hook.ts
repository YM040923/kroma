import { useHeldSeek } from '@kromatv/core/react';
import { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Dimensions, type LayoutChangeEvent } from 'react-native';

import { usePlayerKeys } from '#ui/lib/player/use-player-keys.hook';
import { volumeStep } from '../lib/fmt';
import { chromeMetrics, panelGeometry, scaler, TRANSPORT_HEIGHT } from '../lib/metrics';
import { type ControlId, controlOrder, type PanelHandle } from '../lib/nav';
import { usePanelSlide } from '../lib/panel-slide.hook';
import { DEFAULT_SUB_APPEARANCE } from '../lib/subtitle-appearance';
import { type EpisodeItem, episodeAt, hasEpisodes } from '../parts/episodes-panel';
import { PEEK_HEIGHT, type UpNextData, type UpNextItem } from '../parts/up-next-sheet';
import type { SubtitleGenBundle } from '../parts/settings-panel/settings/gen';
import type { StageFlash } from '../parts/stage-flash';
import { deriveChrome, hasUpNext, initialSettingsView } from '../player-chrome-state';
import { playerInputHandlers } from '../player-input';
import { sortSlots } from '../player-parts';
import type {
  PlayerCloseReason,
  PlayerController,
  PlayerFlags,
  PlayerCloseDetails,
} from '../types';
import { useIdleCursor } from './use-idle-cursor.hook';
import { usePeekGesture } from './use-peek-gesture.hook';
import { usePlayerEnding } from './use-player-ending.hook';
import { usePlayerNav } from './use-player-nav.hook';
import { useResumeNotice } from './use-resume-notice.hook';
import { useSeekNudge } from './use-seek-nudge.hook';
import { useVolumeFlash } from './use-volume-flash.hook';

interface UsePlayerRootArgs {
  controller: PlayerController;
  flags: PlayerFlags;
  active?: boolean;
  onCast?: () => void;
  onClose: (details: PlayerCloseDetails) => void;
  children?: ReactNode;
}

const NO_UP_NEXT: UpNextData = { nextEpisodes: [], recommendations: [] };
const NO_TILE = () => null;
const NO_APPEARANCE_CHANGE = () => undefined;
const NO_SUBTITLE_GEN: SubtitleGenBundle = {
  canCreate: false,
  caps: null,
  pending: [],
  onCancel: () => undefined,
  onDelete: () => undefined,
  onStart: () => undefined,
};

const SKIP_GAP = 24;
const SKIP_REST = 56;

function usePlayerRoot({
  controller,
  flags,
  active,
  onCast,
  onClose,
  children,
}: Readonly<UsePlayerRootArgs>) {
  const c = useHeldSeek(controller);
  const slots = useMemo(() => sortSlots(children), [children]);
  const upNext = slots.upNext?.data ?? NO_UP_NEXT;
  const onPlayItem = slots.upNext?.onPlay;
  const markers = slots.credits?.markers;
  const nextTitle = slots.credits?.next ?? null;
  const onPlayNext = slots.credits?.onPlay;
  // The episode slot is what makes this a series for the chrome: it adds the
  // previous / next pair (drawn together, disabled at the ends) and the
  // season / episode drawer. A film writes no slot and gets neither.
  const episodesData = slots.episodes?.data ?? null;
  const playEpisode = slots.episodes?.onPlay;
  const series = hasEpisodes(episodesData);
  const previousEpisode = hasEpisodes(episodesData) ? episodeAt(episodesData, -1) : null;
  const nextEpisode = hasEpisodes(episodesData) ? episodeAt(episodesData, 1) : null;
  const playPrevious = useCallback(() => {
    if (previousEpisode) {
      playEpisode?.(previousEpisode);
    }
  }, [previousEpisode, playEpisode]);
  const playNextEpisode = useCallback(() => {
    if (nextEpisode) {
      playEpisode?.(nextEpisode);
    }
  }, [nextEpisode, playEpisode]);
  const nextAction = onPlayNext ?? (nextEpisode ? playNextEpisode : undefined);
  const hasNextControl = series || Boolean(onPlayNext);
  const postPlay = slots.postPlay?.item ?? null;
  const onGoHome = slots.postPlay?.onHome;
  const introActive = slots.skipIntro?.active;
  const onSkipIntro = slots.skipIntro?.onSkip;
  const appearance = slots.subtitles?.appearance ?? DEFAULT_SUB_APPEARANCE;
  const onAppearanceChange = slots.subtitles?.onAppearanceChange ?? NO_APPEARANCE_CHANGE;
  const subtitleGen = slots.subtitles?.gen ?? NO_SUBTITLE_GEN;
  const onReport = slots.report?.onReport;
  const rawChapters = slots.transport?.chapters;
  const tileAt = slots.transport?.tileAt ?? NO_TILE;
  // Seeded from the window so the first frame is not measured at zero, then kept
  // honest by the root's own layout. Read once rather than through
  // `useWindowDimensions`, which would subscribe to every resize event.
  const [stageSize, setStageSize] = useState(() => {
    const window = Dimensions.get('window');
    return { width: window.width, height: window.height };
  });
  const onStageLayout = useCallback((e: LayoutChangeEvent) => {
    const { width, height } = e.nativeEvent.layout;
    const next = { width: Math.round(width), height: Math.round(height) };
    setStageSize((prev) =>
      (prev.width === next.width && prev.height === next.height) || next.width <= 0 ? prev : next,
    );
  }, []);
  const [statsOn, setStatsOn] = useState(false);
  const panelRef = useRef<PanelHandle>(null);
  const locked = slots.panel != null;
  const close = useCallback((reason: PlayerCloseReason) => onClose({ reason }), [onClose]);
  const closeFromChrome = useCallback(() => close('close'), [close]);
  const notice = useResumeNotice({
    at: slots.resume?.at,
    cur: c.cur,
    playing: c.playing,
    waiting: c.waiting,
  });
  const startOver = () => {
    notice.dismiss();
    c.seekTo(0);
  };
  // The skip pill waits: it is the same corner as the resume prompt, and the
  // remote belongs to whichever of the two is drawn.
  const intro = useMemo(
    () =>
      onSkipIntro
        ? { active: introActive === true && !notice.shown, onSkip: onSkipIntro }
        : undefined,
    [introActive, onSkipIntro, notice.shown],
  );

  // Measured BEFORE the nav machine, which is then given the row that is drawn:
  // a shed control must not keep a focus stop.
  const row = useMemo(
    () => controlOrder(flags, hasNextControl, { previous: series, episodes: series }),
    [flags, hasNextControl, series],
  );
  const metrics = useMemo(() => chromeMetrics(row, stageSize.width), [row, stageSize.width]);
  const disabledControls = useMemo(() => {
    const out: ControlId[] = [];
    if (series && !previousEpisode) {
      out.push('previous');
    }
    if (hasNextControl && !nextAction) {
      out.push('next');
    }
    return out;
  }, [series, previousEpisode, hasNextControl, nextAction]);
  const px = scaler(metrics.scale);

  const seek = useSeekNudge(c);
  const volumeFlash = useVolumeFlash();
  const volumeMax = c.volumeMax ?? 1;
  const setVolume = (level: number) => {
    c.setVolume(level);
    volumeFlash.show(level);
  };
  const flash: StageFlash | null = seek.burst ? { kind: 'seek', ...seek.burst } : volumeFlash.flash;
  const nav = usePlayerNav(
    c.playing,
    {
      togglePlay: c.togglePlay,
      seekNudge: seek.nudge,
      onNext: () => nextAction?.(),
      onPrevious: series ? playPrevious : undefined,
      hasNext: hasNextControl,
      sheetHasItems: hasUpNext(upNext),
      // A click / OK on the episode key expands the "up next" sheet - an
      // explicit request. The remote's Next media key still calls onNext
      // directly (see lib/nav-keys), so a television keeps its one-press skip.
      nextOpensSheet: flags.pointer,
      volumeNudge: (d) => setVolume(volumeStep(c.volume, d, volumeMax)),
      toggleMute: c.toggleMute,
      togglePip: c.togglePip,
      toggleFullscreen: c.toggleFullscreen,
      onCast,
      onExit: close,
    },
    metrics.controls,
  );

  useIdleCursor(flags.pointer && !nav.revealed);

  const ending = usePlayerEnding({
    controller: c,
    markers,
    postPlay,
    onPlayNext,
    onPlayItem,
    onGoHome,
    onLeave: () => close('ended'),
    onClearOverlay: nav.closeOverlay,
  });
  const credits = ending.credits;

  // The parked "up next" peek is a REQUEST, not a reaction: it opens on a wheel
  // / swipe down or on the episode key, and Back / Escape or a wheel up puts it
  // away before it puts the chrome away. Hovering nothing.
  const [peekArmed, setPeekArmed] = useState(false);
  const dismissPeek = useCallback(() => setPeekArmed(false), []);
  useEffect(() => {
    if (c.playing || ending.over) {
      setPeekArmed(false);
    }
  }, [c.playing, ending.over]);

  usePlayerKeys({
    active,
    nav,
    controller: c,
    flags,
    panelRef,
    locked,
    seekNudge: seek.nudge,
    setVolume,
    resume: { active: notice.shown, onBack: notice.dismiss },
    credits: { active: credits.show, onBack: credits.cancel },
    postPlay: { active: ending.over, onBack: ending.goHome },
    peek: { active: peekArmed && !nav.overlay, onDismiss: dismissPeek },
  });

  const panel = useMemo(() => panelGeometry(stageSize.width), [stageSize.width]);

  const {
    settingsOpen,
    sheetOpen,
    episodesOpen,
    settingsShrink,
    peekVisible,
    chromeShown,
  } = deriveChrome(
    nav,
    upNext,
    panel.covers,
    ending.over,
    peekArmed || !flags.pointer,
    c.playing,
  );
  usePeekGesture({
    open:
      !c.playing &&
      !ending.over &&
      !nav.overlay &&
      nav.revealed &&
      hasUpNext(upNext),
    close: peekArmed && !nav.overlay,
    onOpen: () => setPeekArmed(true),
    onClose: dismissPeek,
  });
  // The drawer must not keep the phone's playback keys away: when the film is
  // STARTED while the drawer is open it gets out of the way. Testing `playing`
  // alone closed the drawer on the very next frame whenever it was opened
  // during playback, which is why the picker looked dead.
  const wasPlaying = useRef(c.playing);
  useEffect(() => {
    const started = !wasPlaying.current && c.playing;
    wasPlaying.current = c.playing;
    if (started && episodesOpen) {
      nav.closeOverlay();
    }
  }, [c.playing, episodesOpen, nav.closeOverlay]);
  const settings = usePanelSlide(settingsOpen);
  const episodes = usePanelSlide(episodesOpen);
  const initialView = initialSettingsView(nav.overlay);
  // Measured rather than assumed, and it falls back to the design height until
  // the first `onLayout` arrives.
  const [transportHeight, setTransportHeight] = useState(0);
  const onTransportLayout = useCallback((e: LayoutChangeEvent) => {
    const height = Math.round(e.nativeEvent.layout.height);
    setTransportHeight((prev) => (prev === height || height <= 0 ? prev : height));
  }, []);
  const transport = transportHeight || px(TRANSPORT_HEIGHT);
  // Reserve the peek's height only when the peek is actually shown: the
  // transport stays docked (28px) in every other state and only lifts to make
  // room for the peek once the viewer explicitly asks for it.
  const bottomInset = peekVisible ? PEEK_HEIGHT : px(28);
  const clearance = bottomInset + transport + px(SKIP_GAP);
  const introLift = chromeShown ? clearance : px(SKIP_REST);
  const cueClearance = chromeShown ? clearance : 0;
  const input = playerInputHandlers(nav, c, flags, locked);

  const { openOverlay, closeOverlay } = nav;
  const openSheet = useCallback(() => openOverlay('sheet'), [openOverlay]);
  const closeSheet = useCallback(() => {
    closeOverlay();
    setPeekArmed(false);
  }, [closeOverlay]);
  // Close first: pip, cast and the next episode all change what is on screen, and
  // leaving the panel over it would hide the thing just asked for.
  const runOverflow = (id: ControlId) => {
    nav.closeOverlay();
    nav.activate(id);
  };
  const playUpNextItem = useCallback((item: UpNextItem) => onPlayItem?.(item), [onPlayItem]);
  const playEpisodeItem = useCallback(
    (item: EpisodeItem) => {
      // Close first: the chosen episode replaces what is on screen, and leaving
      // the drawer over it would hide the thing just asked for.
      closeOverlay();
      playEpisode?.(item);
    },
    [closeOverlay, playEpisode],
  );
  const closeEpisodes = useCallback(() => closeOverlay(), [closeOverlay]);
  const seekNudge = (dir: -1 | 1) => seek.nudge(dir, 'hold');
  const volumeNudge = (dir: -1 | 1) => setVolume(volumeStep(c.volume, dir, volumeMax));

  return {
    c,
    slots,
    upNext,
    nextTitle,
    onPlayNext,
    postPlay,
    appearance,
    onAppearanceChange,
    subtitleGen,
    onReport,
    rawChapters,
    tileAt,
    stageSize,
    onStageLayout,
    statsOn,
    setStatsOn,
    panelRef,
    locked,
    closeFromChrome,
    notice,
    startOver,
    intro,
    metrics,
    disabledControls,
    flash,
    nav,
    credits,
    ending,
    input,
    settingsShrink,
    settings,
    initialView,
    panel,
    onTransportLayout,
    transport,
    bottomInset,
    introLift,
    cueClearance,
    sheetOpen,
    peekVisible,
    chromeShown,
    openSheet,
    closeSheet,
    runOverflow,
    playUpNextItem,
    series,
    episodesData,
    episodes,
    episodesOpen,
    playEpisodeItem,
    closeEpisodes,
    seekNudge,
    volumeNudge,
  };
}

export { usePlayerRoot };
