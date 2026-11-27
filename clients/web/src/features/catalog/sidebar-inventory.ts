import { hasPermission, type User } from '@kromatv/client/accounts';
import type { Library } from '@kromatv/client/media';
import type { Translate } from '@kromatv/i18n';
import { isAnyAdmin, type ModuleNav } from '@kromatv/module-sdk';
import type { IconName } from '@kromatv/ui/kit';
import { resolveModuleIcon } from '#web/modules/module-icons';
import { CATALOG_NAV } from './catalog-nav';
import { LIBRARY_ICONS, libraryPreferenceId, navigationPreferenceId } from './library-identity';

export interface SidebarEntry {
  id: string;
  title: string;
  icon: IconName;
  to: string;
  params?: { id: string };
  exact?: boolean;
  expandedOnly?: boolean;
}

export interface SidebarGroup {
  id: 'discover' | 'libraries' | 'space' | 'more';
  label: string;
  items: SidebarEntry[];
}

export const RELEASE_LINK = 'releases.title';

export function sidebarInventory(
  t: Translate,
  user: Pick<User, 'permissions'> | null,
  libraries: readonly Library[],
  modules: readonly ModuleNav[],
): SidebarGroup[] {
  if (!user) return [];
  const more: SidebarEntry[] = [];
  if (hasPermission(user, 'users.manage')) more.push({ id: 'nav.inviteUser', title: t('nav.inviteUser'), to: '/invite', icon: 'user-plus', expandedOnly: true });
  more.push({ id: 'nav.connectDevice', title: t('nav.connectDevice'), to: '/connect', icon: 'device-desktop', expandedOnly: true });
  if (isAnyAdmin(user)) more.push({ id: 'nav.server', title: t('nav.server'), to: '/admin', icon: 'settings' });
  more.push({ id: RELEASE_LINK, title: t('releases.title'), to: '/whats-new', icon: 'sparkles', expandedOnly: true });
  return [
    { id: 'discover', label: t('nav.discover'), items: CATALOG_NAV.map(({ labelKey, ...item }) => ({ id: labelKey, title: t(labelKey), ...item })) },
    { id: 'libraries', label: t('account.sidebarLibraries'), items: libraries.map((lib) => ({ id: libraryPreferenceId(lib.id), title: lib.name, icon: LIBRARY_ICONS[lib.kind], to: '/libraries/$id', params: { id: lib.id }, exact: true })) },
    { id: 'space', label: t('nav.yourSpace'), items: [
      { id: 'nav.myList', title: t('nav.myList'), to: '/my-list', icon: 'bookmark' },
      ...modules.map(n => ({ id: navigationPreferenceId('module', JSON.stringify([n.moduleId, n.to])), title: n.label, to: n.to, icon: resolveModuleIcon(n.icon) })),
    ] },
    { id: 'more', label: t('nav.more'), items: more },
  ];
}
