// The episode drawer: every season of the show, the playable episodes in the
// chosen one, and where the viewer is in it. A side panel rather than a
// full-stage dialog, and one rung BELOW the chrome in z-order: on a phone the
// drawer may take most of the width, but play/pause and the seek bar stay on
// screen and stay clickable. It leaves as soon as an episode is picked.

import { type Ref, useCallback, useEffect, useImperativeHandle, useMemo, useState } from 'react';
import { Animated, Pressable } from 'react-native';
import { Box } from '#ui/components/atoms/box';
import { Chip } from '#ui/components/atoms/chip';
import { Focusable } from '#ui/components/atoms/focusable';
import { Img } from '#ui/components/atoms/img';
import { Progress } from '#ui/components/atoms/progress';
import { Text } from '#ui/components/atoms/text';
import type { PanelHandle } from '#ui/components/organisms/player/lib/nav';
import { FOCUS_SCALE } from '#ui/components/organisms/player/lib/style';
import { IconOk } from '#ui/components/organisms/player/parts/icons.component';
import { TitleBar } from '#ui/components/organisms/player/parts/settings-panel/settings/title-bar.component';
import { styles } from '#ui/core';
import { useStyles } from '#ui/core/styles';
import { NO_OUTLINE } from '#ui/lib/field-shell';
import { FocusScroll } from '#ui/lib/focus-scroll.component';
import {
  FocusRegion,
  FocusScope,
  useLockFocusBehind,
} from '#ui/lib/focus-scope.component';
import { VIRTUAL_FOCUS } from '#ui/lib/player/virtual-focus';
import { usePanelTravel } from '#ui/lib/player/panel-travel.hook';
import { useT } from '#ui/services/i18n';
import {
  currentSeason,
  type EpisodeItem,
  type EpisodesData,
  type SeasonItem,
} from './episodes.types';

export interface EpisodesPanelProps {
  data: EpisodesData;
  /** False while the drawer is leaving: it stays mounted for the exit. */
  shown?: boolean;
  /** The drawer's width in px. The host narrows it on a phone so a strip of
   *  the picture stays visible. */
  width?: number;
  /** The chrome's scale (see ../../lib/metrics). */
  scale?: number;
  /** Pixels the chrome occupies at the foot of the stage. The list gives them
   *  back as padding so the last row is not buried under the transport. */
  bottomInset?: number;
  /** The playing episode's live position, 0..100, which beats a stored one. */
  currentProgress?: number | null;
  onPlay: (item: EpisodeItem) => void;
  onClose: () => void;
  ref?: Ref<PanelHandle>;
}

// The width a row's still is actually drawn at, so the host asks the artwork
// proxy for that size rather than a full backdrop per episode.
export const EPISODE_ART_W = 112;
const ART_H = 63;

function pctOf(item: EpisodeItem, currentId: string, currentProgress?: number | null) {
  const value =
    item.id === currentId && currentProgress != null ? currentProgress : (item.progress ?? null);
  if (value == null || !Number.isFinite(value)) {
    return null;
  }
  return Math.min(100, Math.max(0, Math.round(value)));
}

function EpisodeRow({
  item,
  current,
  progress,
  onPlay,
  px,
}: Readonly<{
  item: EpisodeItem;
  current: boolean;
  progress: number | null;
  onPlay: (item: EpisodeItem) => void;
  px: (n: number) => number;
}>) {
  const s = useStyles(sDefinition);

  const t = useT();
  const playing = current;
  return (
    <Focusable
      label={`${item.episode}. ${item.title}`}
      onPress={() => onPlay(item)}
      autoFocus={playing}
      focusScale={FOCUS_SCALE}
      selected={playing}
      style={[s.row, playing ? s.rowCurrent : null, NO_OUTLINE]}
    >
      <Box row align="center" gap={px(14)}>
        <Box w={px(EPISODE_ART_W)} h={px(ART_H)} radius="md" overflow="hidden" bg="surface2">
          <Img src={item.artUrl ?? null} fill />
          {item.watched ? (
            <Box absolute right={5} bottom={5} radius="circle" bg="accent" p={3}>
              <IconOk size={12} color="accentInk" />
            </Box>
          ) : null}
        </Box>
        <Box flex gap={2}>
          <Text lines={1} style={s.title} color={playing ? 'accentText' : 'text'}>
            {item.episode}. {item.title}
          </Text>
          <Box row align="center" gap={8}>
            {item.subtitle ? (
              <Text lines={1} color="textMuted" style={s.meta}>
                {item.subtitle}
              </Text>
            ) : null}
            {playing ? (
              <Text color="accentText" style={s.meta}>
                {t('player.nowPlaying')}
              </Text>
            ) : item.watched ? (
              <Text color="textMuted" style={s.meta}>
                {t('content.watched')}
              </Text>
            ) : progress != null && progress > 0 ? (
              <Text color="textMuted" style={s.meta}>
                {t('player.episodeProgress', { percent: progress })}
              </Text>
            ) : null}
          </Box>
        </Box>
      </Box>
      {!item.watched && progress != null && progress > 0 ? (
        <Box mt={px(10)}>
          <Progress value={progress / 100} thickness={3} />
        </Box>
      ) : null}
    </Focusable>
  );
}

/** One season's episodes, in episode order. */
function seasonEpisodes(season: SeasonItem): EpisodeItem[] {
  return season.episodes.slice().sort((a, b) => a.episode - b.episode);
}

export function EpisodesPanel({
  data,
  shown = true,
  width = 560,
  scale = 1,
  bottomInset = 0,
  currentProgress,
  onPlay,
  onClose,
  ref,
}: Readonly<EpisodesPanelProps>) {
  const s = useStyles(sDefinition);

  const t = useT();
  const px = useCallback((n: number) => Math.round(n * scale), [scale]);
  const travel = usePanelTravel(shown, width);

  const [season, setSeason] = useState(() => currentSeason(data));
  const playingSeason = currentSeason(data);
  useEffect(() => {
    setSeason(playingSeason);
  }, [playingSeason]);
  const picked = useMemo(
    () => data.seasons.find((one) => one.number === season) ?? data.seasons[0],
    [data.seasons, season],
  );

  useLockFocusBehind(shown);
  useImperativeHandle(
    ref,
    () => ({
      onKey: (key) => {
        if (key !== 'Back') {
          return false;
        }
        onClose();
        return true;
      },
    }),
    [onClose],
  );

  return (
    <>
      <Pressable
        {...VIRTUAL_FOCUS}
        accessibilityRole="button"
        accessibilityLabel={t('common.close')}
        onPress={onClose}
        style={[s.scrim, shown ? s.scrimLive : s.scrimInert]}
      />
      <Animated.View style={[s.panel, { width }, travel]}>
        <FocusScope bridge={false} active={shown} entryKey={`${season}`}>
          <FocusScroll
            style={s.scroll}
            contentStyle={{
              paddingHorizontal: px(40),
              paddingTop: px(36),
              paddingBottom: bottomInset + px(40),
            }}
            reveal="nearest"
            offsetFromStart={px(12)}
          >
            <TitleBar title={t('player.episodesPicker')} onClose={onClose} px={px} />
            <Text lines={1} color="textMuted" style={s.show}>
              {data.showTitle}
            </Text>

            {data.seasons.length > 1 ? (
              <FocusRegion style={s.seasons}>
                {data.seasons.map((one) => (
                  <Chip
                    key={one.number}
                    variant="subtle"
                    active={one.number === season}
                    label={t('content.season', { number: one.number })}
                    count={one.episodes.length}
                    onPress={() => setSeason(one.number)}
                  />
                ))}
              </FocusRegion>
            ) : null}

            {picked ? (
              <Box gap={px(8)} mt={px(18)}>
                {seasonEpisodes(picked).map((item) => (
                  <EpisodeRow
                    key={item.id}
                    item={item}
                    current={item.id === data.currentId}
                    progress={pctOf(item, data.currentId, currentProgress)}
                    onPlay={onPlay}
                    px={px}
                  />
                ))}
              </Box>
            ) : null}
          </FocusScroll>
        </FocusScope>
      </Animated.View>
    </>
  );
}

const sDefinition = styles({
  scrim: { position: 'absolute', top: 0, bottom: 0, left: 0, right: 0, zIndex: 13 },
  scrimLive: { pointerEvents: 'auto', opacity: 1 },
  scrimInert: { pointerEvents: 'none', opacity: 0 },
  // One rung under the chrome (z 20): the transport stays drawn and clickable
  // with the drawer open, which is what keeps a phone's playback keys at hand.
  panel: {
    position: 'absolute',
    top: 0,
    bottom: 0,
    right: 0,
    zIndex: 14,
    bg: 'playerSurface/94',
    border: 'border',
  },
  scroll: { flex: true, minH: 0 },
  show: { font: 'ui', fontWeight: '500', mb: 14 },
  seasons: { row: true, wrap: true, gap: 8, mb: 4 },
  row: { p: 10, radius: 'md', bg: 'transparent' },
  rowCurrent: { bg: 'accentSoft' },
  title: { font: 'ui', fontWeight: '700' },
  meta: { font: 'ui', fontSize: 12, fontWeight: '500' },
});
