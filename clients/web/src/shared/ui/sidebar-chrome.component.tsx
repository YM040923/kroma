import { useT } from '@kromatv/ui';
import { Box, Drawer, IconButton, Row, styles, useStyles } from '@kromatv/ui/kit';
import { useRouterState } from '@tanstack/react-router';
import { type ReactNode, useState } from 'react';
import type { ViewStyle } from 'react-native';
import { safeAreaTop } from '#web/shared/lib/safe-area';
import type { SidebarMode } from '#web/shared/lib/sidebar-collapsed.hook';

// The two pieces of sidebar chrome both shells share: the fold / hide controls
// a visible rail carries, and the floating launcher that stands in once the
// rail is hidden. Stated once beside `useSidebarMode`, for the same reason the
// topbars share `NavMenuButton`: the catalogue and the console may not reach
// into each other.

const sDefinition = styles({
  launcher: {
    position: 'fixed',
    top: 0,
    left: 0,
    zIndex: 30,
    paddingTop: 'max(8px, env(safe-area-inset-top))',
    paddingLeft: 'max(4px, env(safe-area-inset-left))',
  },
  drawerHead: { pl: 20, pr: 12, pb: 12, ...safeAreaTop(20) },
});

/** One control for the three states: full → rail → hidden → full. The glyph
 *  and its fill change with the state, so the button always says what the next
 *  press does instead of standing as three separate keys. */
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

function LauncherButton({ open, onPress }: Readonly<{ open: boolean; onPress: () => void }>) {
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
        label={t('nav.menu')}
        expanded={open}
        onPress={onPress}
      />
    </Box>
  );
}

function SheetHeader({ brand, onExpand }: Readonly<{ brand: ReactNode; onExpand: () => void }>) {
  const t = useT();
  const s = useStyles(sDefinition);
  return (
    <Drawer.Header style={s.drawerHead}>
      <Row between>
        {brand}
        <Row gap={2}>
          <IconButton
            variant="ghost"
            icon="chevron-right"
            label={t('nav.expandSidebar')}
            onPress={onExpand}
          />
          <Drawer.Close variant="glass" diameter={40} glyph={20} radius="md" />
        </Row>
      </Row>
    </Drawer.Header>
  );
}

interface LauncherProps {
  brand: ReactNode;
  panelStyle?: ViewStyle;
  onExpand: () => void;
  children: ReactNode;
}

/** What stands in for a hidden rail: one fixed control over the content's
 *  top-left corner, and the navigation it opens as an overlay sheet. The sheet
 *  closes itself on navigation and never reflows the page behind it. */
export function HiddenSidebarLauncher({ brand, panelStyle, onExpand, children }: LauncherProps) {
  const [open, setOpen] = useState(false);
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const [openedAt, setOpenedAt] = useState(pathname);
  if (openedAt !== pathname) {
    setOpenedAt(pathname);
    setOpen(false);
  }
  const expand = () => {
    onExpand();
    setOpen(false);
  };
  return (
    <>
      <LauncherButton open={open} onPress={() => setOpen(true)} />
      <Drawer.Root
        open={open}
        onClose={() => setOpen(false)}
        title="KROMA"
        side="left"
        width="xs"
        fullBelow={640}
        pad={0}
        panelStyle={panelStyle}
      >
        <SheetHeader brand={brand} onExpand={expand} />
        <Drawer.Panel>{children}</Drawer.Panel>
      </Drawer.Root>
    </>
  );
}
