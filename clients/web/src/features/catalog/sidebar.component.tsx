import { useT } from '@kromatv/ui';
import { Box, Button, classes, Drawer, Logo, Row, styles, Text, useBreakpoint, useStyles } from '@kromatv/ui/kit';
import { Link, useRouterState } from '@tanstack/react-router';
import { useState } from 'react';
import { ScrollView } from 'react-native';
import { UserChip } from '#web/features/catalog/sidebar-account.component';
import { RELEASE_LINK, type SidebarGroup } from '#web/features/catalog/sidebar-inventory';
import { SidebarVersion } from '#web/features/catalog/sidebar-version.component';
import { ServerUpdateIndicator, ServerUpdateNotice } from '#web/features/catalog/server-update-notice.component';
import { useSidebarInventory } from '#web/features/catalog/use-sidebar-inventory.hook';
import { WhatsNewLink } from '#web/features/catalog/whats-new-link.component';
import { useSidebarMode } from '#web/shared/lib/sidebar-collapsed.hook';
import { useNavActions } from '#web/shared/ui/nav-actions';
import { NavMenuButton } from '#web/shared/ui/nav-menu-button.component';
import { SideNav, SideNavCompactProvider } from '#web/shared/ui/side-nav.component';
import { SIDE_NAV_FRAME, SIDE_NAV_GUTTER } from '#web/shared/ui/side-nav-style';
import { HiddenSidebarLauncher, SidebarModeControls } from '#web/shared/ui/sidebar-chrome.component';

const SIDEBAR_WIDTH = 248;
const SIDEBAR_RAIL_WIDTH = 76;

const sDefinition = styles({
  sidebar: {
    position: 'sticky', top: 0, height: '100vh', display: 'flex', flexDirection: 'column',
    borderWidth: 0, borderRightWidth: 1, borderStyle: 'solid',
    borderRightColor: 'var(--kroma-border)', backgroundColor: 'var(--kroma-surface-1)',
  },
  topbar: {
    position: 'sticky', top: 0, zIndex: 40, display: 'flex', alignItems: 'center', justifyContent: 'space-between',
    borderWidth: 0, borderBottomWidth: 1, borderStyle: 'solid',
    borderBottomColor: 'var(--kroma-border)', bg: 'bg/85', px: 16, pb: 10,
    paddingTop: 'max(0.625rem, env(safe-area-inset-top))',
    backdropFilter: 'blur(16px) saturate(1.4)', WebkitBackdropFilter: 'blur(16px) saturate(1.4)',
  },
  drawerHead: { px: SIDE_NAV_GUTTER, paddingTop: 'max(1.75rem, env(safe-area-inset-top))', pb: 8 },
  logoLink: { display: 'block', color: 'inherit', textDecorationLine: 'none' },
  navFill: { bg: 'surface1' },
  scrollContent: { flexGrow: 1 },
  accountArea: { pt: 12, mt: 12, borderTopWidth: 1, borderTopColor: 'border' },
});

export function Sidebar() {
  const s = useStyles(sDefinition);
  const [mode, setMode] = useSidebarMode();
  const step = useBreakpoint();
  if (step !== 'lg' && step !== 'tv') return null;
  if (mode === 'hidden') return <HiddenSidebarLauncher onExpand={() => setMode('expanded')} />;
  const compact = mode === 'rail';
  return (
    <aside className={classes(s.sidebar)} style={{ width: compact ? SIDEBAR_RAIL_WIDTH : SIDEBAR_WIDTH }}>
      <SideNavCompactProvider compact={compact}>
        <SideNav.Header compact={compact} toggle={<SidebarModeControls compact={compact} onChange={setMode} />}>
          <Logo size={compact ? 26 : 24} markOnly={compact} />
        </SideNav.Header>
        <ScrollView style={SIDE_NAV_FRAME} contentContainerStyle={s.scrollContent} showsVerticalScrollIndicator={false}>
          <SidebarNav collapsed={compact} />
        </ScrollView>
      </SideNavCompactProvider>
    </aside>
  );
}

function GroupLinks({ group, collapsed, hidden }: Readonly<{ group: SidebarGroup; collapsed: boolean; hidden: Set<string> }>) {
  const visible = group.items.filter(item => !hidden.has(item.id) && !(collapsed && item.expandedOnly));
  if (!visible.length) return null;
  if (visible.length === 1 && visible[0]?.id === RELEASE_LINK) return <WhatsNewLink groupLabel={group.label} />;
  return <SideNav.Group label={group.label}>
    {visible.map(item => {
      if (item.id === RELEASE_LINK) return <WhatsNewLink key={item.id} />;
      const link = <SideNav.Item key={item.id} to={item.to} params={item.params} exact={item.exact} icon={item.icon} label={item.title}>
        <SideNav.Label>{item.title}</SideNav.Label>
      </SideNav.Item>;
      return collapsed ? <div key={item.id} title={item.title}>{link}</div> : link;
    })}
  </SideNav.Group>;
}

function SidebarNav({ collapsed = false }: Readonly<{ collapsed?: boolean }>) {
  const s = useStyles(sDefinition);
  const t = useT();
  const { groups, layout } = useSidebarInventory();
  const hidden = new Set(layout.data?.hidden);
  const section = (group: SidebarGroup) => <GroupLinks key={group.id} group={group} collapsed={collapsed} hidden={hidden} />;
  return (
    <SideNav.Root>
      {layout.data && groups.filter(group => group.id !== 'more').map(section)}
      {layout.isError && !layout.data ? <Box gap={6} align="center">
        {collapsed ? null : <Text variant="meta" color="textMuted">{t('account.sidebarLoadFailed')}</Text>}
        <Button variant="ghost" size="sm" label={t('error.retry')} onPress={() => void layout.refetch()} />
      </Box> : null}
      <SideNav.Footer>
        {collapsed ? null : <ServerUpdateNotice />}
        {layout.data && groups.filter(group => group.id === 'more').map(section)}
        <Box style={s.accountArea}>
          <UserChip compact={collapsed} />
          {collapsed ? null : <Box px={4} pt={6}><SidebarVersion /></Box>}
        </Box>
      </SideNav.Footer>
    </SideNav.Root>
  );
}

export function MobileTopbar() {
  const s = useStyles(sDefinition);
  const actions = useNavActions();
  const [open, setOpen] = useState(false);
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const step = useBreakpoint();
  const [openedAt, setOpenedAt] = useState(pathname);
  if (openedAt !== pathname) {
    setOpenedAt(pathname);
    setOpen(false);
  }
  if (step === 'lg' || step === 'tv') return null;
  return (
    <header className={classes(s.topbar)}>
      <Box row align="center" gap={8}>
        <NavMenuButton open={open} onPress={() => setOpen(true)} />
        <Link to="/" aria-label="KROMA" className={classes(s.logoLink)}><Logo size={20} /></Link>
        <Drawer.Root open={open} onClose={() => setOpen(false)} title="KROMA" side="left" width="xs" fullBelow={640} pad={0} panelStyle={s.navFill}>
          <Drawer.Header style={s.drawerHead}>
            <Row between>
              <Box px={8} pb={8}><Logo size={24} /></Box>
              <Drawer.Close variant="glass" diameter={40} glyph={20} radius="md" />
            </Row>
          </Drawer.Header>
          <Drawer.Panel showsVerticalScrollIndicator={false}><SidebarNav /></Drawer.Panel>
        </Drawer.Root>
      </Box>
      <Box row align="center" gap={2}><ServerUpdateIndicator />{actions}</Box>
    </header>
  );
}
