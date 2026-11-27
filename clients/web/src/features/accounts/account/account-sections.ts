export const ACCOUNT_SECTIONS = [
  'profile',
  'home',
  'sidebar',
  'preferences',
  'notifications',
  'security',
  'devices',
] as const;

export type AccountSection = (typeof ACCOUNT_SECTIONS)[number];
