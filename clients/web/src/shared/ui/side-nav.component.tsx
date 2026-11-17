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

function Header({
  children,
  compact = false,
  toggle,
}: Readonly<{ children: ReactNode; compact?: boolean; toggle?: ReactNode }>) {
  const actions = useNavActions();
  return (
    <Box
      shrink={0}
      align={compact ? 'center' : undefined}
      gap={compact ? 4 : 10}
      pl={compact ? 6 : SIDE_NAV_BAND_X}
      pr={compact ? 6 : SIDE_NAV_GUTTER}
      pt={compact ? 18 : 24}
      pb={compact ? 12 : 16}
    >
      {compact ? (
        <>
          {toggle}
          <Box align="center" my={2}>
            {children}
          </Box>
          <Box align="center" gap={4}>
            {actions}
          </Box>
        </>
      ) : (
        <>
          <Box row align="center" justify="space-between">
            {children}
            <Box row align="center" gap={4}>
              {toggle}
            </Box>
          </Box>
          <Box row align="center" gap={2}>
            {actions}
          </Box>
        </>
      )}
    </Box>
  );
}

interface SideNavRootProps {
  children: ReactNode;
}

function Root({ children }: Readonly<SideNavRootProps>) {
  const compact = useContext(CompactContext);
  return (
    <Box role="navigation" flex minH={0} px={compact ? 6 : SIDE_NAV_GUTTER} pt={4}>
      {children}
    </Box>
  );
}

/** A rail row: the glyph centred in a square that reads as one target, while
 *  the expanded row keeps its label-led layout. */
const COMPACT_ROW = {
  width: 56,
  minHeight: 44,
  paddingHorizontal: 0,
  justifyContent: 'center',
  marginLeft: 'auto',
  marginRight: 'auto',
} as const;

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
  const compact = useContext(CompactContext);
  const current = matchRoute({ to, fuzzy: !exact }) !== false;
  return (
    <Focusable
      sv={sideNavRow}
      vars={{ current }}
      current={current ? 'page' : undefined}
      disabled={disabled}
      style={compact ? COMPACT_ROW : undefined}
      asChild
    >
      {({ slots }) => (
        <RouteLink to={to}>
          <RowContext.Provider value={slots}>
            <Icon
              name={icon}
              size={compact ? 20 : SIDE_NAV_GLYPH}
              thickness={1.7}
              color={slots.glyph.color}
            />
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
  const compact = useContext(CompactContext);
  if (compact) {
    return null;
  }
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
