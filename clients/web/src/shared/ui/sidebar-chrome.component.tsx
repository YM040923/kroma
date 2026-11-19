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
    paddingTop: 'max(12px, env(safe-area-inset-top))',
    paddingLeft: 'max(12px, env(safe-area-inset-left))',
  },
  drawerHead: { pl: 20, pr: 12, pb: 12, ...safeAreaTop(20) },
});

/** Fold / unfold the rail, and dismiss it. In the rail the two stack so both
 *  stay reachable inside the folded column. */
export function SidebarModeControls({
  compact,
  onChange,
}: Readonly<{ compact: boolean; onChange: (mode: SidebarMode) => void }>) {
  const t = useT();
  return (
    <Box row={!compact} gap={compact ? 4 : 2} align="center">
      <IconButton
        variant="ghost"
        diameter={compact ? 28 : undefined}
        icon={compact ? 'chevron-right' : 'chevron-left'}
        label={t(compact ? 'nav.expandSidebar' : 'nav.collapseSidebar')}
        onPress={() => onChange(compact ? 'expanded' : 'rail')}
      />
      <IconButton
        variant="ghost"
        diameter={compact ? 28 : undefined}
        icon="arrow-bar-to-left"
        label={t('nav.hideSidebar')}
        onPress={() => onChange('hidden')}
      />
    </Box>
  );
}

function LauncherButton({ open, onPress }: Readonly<{ open: boolean; onPress: () => void }>) {
  const t = useT();
  const s = useStyles(sDefinition);
  return (
    <Box style={s.launcher}>
      <IconButton
        variant="glass"
        diameter={40}
        radius="md"
        glyph={21}
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
