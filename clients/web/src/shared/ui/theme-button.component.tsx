import { useT } from '@kromatv/ui';
import { IconButton, readMode, type ThemeMode, useStyleRuntime, writeMode } from '@kromatv/ui/kit';
import { useEffect, useState } from 'react';

const ICON = { system: 'device-desktop', light: 'sun', dark: 'moon' } as const;
const ORDER: readonly ThemeMode[] = ['system', 'light', 'dark'];

/** The appearance switch on the top bar: one press steps to the next mode, so
 *  system → light → dark → system cycles without opening a menu. */
export function ThemeButton() {
  const t = useT();
  const runtime = useStyleRuntime();
  const [mode, setMode] = useState<ThemeMode>('system');

  useEffect(() => {
    setMode(readMode());
  }, []);

  const next = ORDER[(ORDER.indexOf(mode) + 1) % ORDER.length] ?? 'system';

  return (
    <IconButton
      icon={ICON[mode]}
      variant="ghost"
      diameter={40}
      radius="md"
      label={`${t('appearance.title')}：${t(`appearance.${mode}`)}`}
      onPress={() => {
        setMode(next);
        writeMode(next, runtime);
      }}
    />
  );
}
