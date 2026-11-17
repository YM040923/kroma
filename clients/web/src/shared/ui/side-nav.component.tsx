import { Box, Focusable, Icon, type IconName, Text } from '@kromatv/ui/kit';
import { useMatchRoute } from '@tanstack/react-router';
import { createContext, type ReactNode, useContext } from 'react';
import { useNavActions } from '#web/shared/ui/nav-actions';
import { RouteLink } from '#web/shared/ui/route-link.component';
import {
  SIDE_NAV_BAND_X,
  SIDE_NAV_GLYPH,
  SIDE_NAV_GUTTER,
  SIDE_NAV_SAFE_BOTTOM,
  sideNavRow,
} from '#web/shared/ui/side-nav-style';

type RowSlots = ReturnType<typeof sideNavRow>;

const RowContext = createContext<RowSlots | null>(null);
/** A folded rail: glyphs only. Every label below it renders nothing, so a page
 *  that contributes a nav item needs no collapsed variant of its own. */
const CompactContext = createContext(false);

export function SideNavCompactProvider({
  compact,
  children,
}: Readonly<{ compact: boolean; children: ReactNode }>) {
  return <CompactContext.Provider value={compact}>{children}</CompactContext.Provider>;
}

function Header({ children, compact = false }: Readonly<{ children: ReactNode; compact?: boolean }>) {
  const actions = useNavActions();
  return (
    <Box
      shrink={0}
      row={!compact}
      align="center"
      justify={compact ? 'center' : undefined}
      between={!compact}
      gap={compact ? 12 : 8}
      pl={compact ? 8 : SIDE_NAV_BAND_X}
      pr={compact ? 8 : SIDE_NAV_GUTTER}
      pt={24}
      pb={16}
    >
      {children}
      {actions}
    </Box>
  );
}

interface SideNavRootProps {
  children: ReactNode;
}

function Root({ children }: Readonly<SideNavRootProps>) {
  const compact = useContext(CompactContext);
  return (
    <Box role="navigation" flex minH={0} px={compact ? 8 : SIDE_NAV_GUTTER} pt={4}>
      {children}
    </Box>
  );
}

interface SideNavGroupProps {
  label: string;
  children: ReactNode;
}

function Group({ label, children }: Readonly<SideNavGroupProps>) {
  const compact = useContext(CompactContext);
  return (
    <Box pt={compact ? 10 : 18}>
      {compact ? null : (
        <Text variant="overline" color="textDim" px={SIDE_NAV_GUTTER} pb={6}>
          {label}
        </Text>
      )}
      {children}
    </Box>
  );
}

interface SideNavItemProps {
  to: string;
  exact?: boolean;
  icon: IconName;
  disabled?: boolean;
  children: ReactNode;
}

function Item({ to, exact = false, icon, disabled = false, children }: Readonly<SideNavItemProps>) {
  const matchRoute = useMatchRoute();
  const current = matchRoute({ to, fuzzy: !exact }) !== false;
  return (
    <Focusable
      sv={sideNavRow}
      vars={{ current }}
      current={current ? 'page' : undefined}
      disabled={disabled}
      asChild
    >
      {({ slots }) => (
        <RouteLink to={to}>
          <RowContext.Provider value={slots}>
            <Icon name={icon} size={SIDE_NAV_GLYPH} thickness={1.7} color={slots.glyph.color} />
            {children}
          </RowContext.Provider>
        </RouteLink>
      )}
    </Focusable>
  );
}

function Label({ children }: Readonly<{ children: ReactNode }>) {
  const slots = useContext(RowContext);
  const compact = useContext(CompactContext);
  if (compact) {
    return null;
  }
  return (
    <Text variant="label" lines={1} style={slots?.label}>
      {children}
    </Text>
  );
}

function Trailing({ children }: Readonly<{ children: ReactNode }>) {
  return (
    <Box row align="center" gap={6} shrink={0} ml="auto">
      {children}
    </Box>
  );
}

function Footer({ children }: Readonly<{ children: ReactNode }>) {
  return (
    <Box mt="auto" pt={24} style={SIDE_NAV_SAFE_BOTTOM}>
      {children}
    </Box>
  );
}

const SideNav = { Header, Root, Group, Item, Label, Trailing, Footer };

export { SideNav };
