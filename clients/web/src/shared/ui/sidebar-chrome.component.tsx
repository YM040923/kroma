import { useT } from '@kromatv/ui';
import { Box, IconButton, styles, useStyles } from '@kromatv/ui/kit';
import type { SidebarMode } from '#web/shared/lib/sidebar-collapsed.hook';

// The two pieces of sidebar chrome both shells share: the fold control a
// visible rail carries, and the floating launcher that brings the rail back
// once it is hidden. Stated once beside `useSidebarMode`, for the same reason
// the topbars share `NavMenuButton`: the catalogue and the console may not
// reach into each other.

const sDefinition = styles({
  launcher: {
    position: 'fixed',
    top: 0,
    left: 0,
    zIndex: 30,
    paddingTop: 'max(8px, env(safe-area-inset-top))',
    paddingLeft: 'max(4px, env(safe-area-inset-left))',
  },
});

/** One control for the three states: expanded → rail → hidden → expanded.
 *  The glyph and its fill change with the state, so the button always says
 *  what the next press does instead of standing as three separate keys. */
export function SidebarModeControls({
  compact,
  onChange,
}: Readonly<{ compact: boolean; onChange: (mode: SidebarMode) => void }>) {
  const t = useT();
  const next: SidebarMode = compact ? 'hidden' : 'rail';
  return (
    <IconButton
      variant={compact ? 'glass' : 'ghost'}
      diameter={compact ? 30 : undefined}
      icon={compact ? 'arrow-bar-to-left' : 'chevron-left'}
      label={t(compact ? 'nav.hideSidebar' : 'nav.collapseSidebar')}
      onPress={() => onChange(next)}
    />
  );
}

/** What stands in for a hidden rail: one fixed control that brings the same
 *  rail back. No sheet in between — hidden is a stop of the cycle, not a
 *  fourth mode. */
export function HiddenSidebarLauncher({ onExpand }: Readonly<{ onExpand: () => void }>) {
  const t = useT();
  const s = useStyles(sDefinition);
  return (
    <Box style={s.launcher}>
      <IconButton
        variant="glass"
        diameter={34}
        radius="md"
        glyph={18}
        icon="menu-2"
        label={t('nav.expandSidebar')}
        onPress={onExpand}
      />
    </Box>
  );
}