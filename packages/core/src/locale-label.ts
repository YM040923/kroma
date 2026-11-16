import type { Locale, OfferedLocale, Translate } from '@kromatv/i18n';

// `lang.<code>` exists for a language the source catalog names; a regional one
// (`zh-Hans` above `zh`) reads its parent's name rather than asking for a key
// no catalog has, which the typed translator refuses.
export function localeLabel(t: Translate, locale: OfferedLocale<Locale>): string {
  const code = String(locale.code);
  const [language] = code.split('-');
  const key = code === language ? `lang.${code}` : `lang.${language ?? code}`;
  let name: string;
  try {
    name = t(key as Parameters<Translate>[0]);
  } catch {
    name = code;
  }
  return locale.stage === 'beta' ? t('common.languageBeta', { name }) : name;
}
