import { useT } from '@kromatv/ui';
import { Menu, readMode, type ThemeMode, useStyleRuntime, writeMode } from '@kromatv/ui/kit';
import { useEffect, useState } from 'react';

const ICON = { system: 'device-desktop', light: 'sun', dark: 'moon' } as const;

/** The appearance choice, on the top bar itself: a global switch that only
 *  lived in preferences is a switch nobody finds. */
export function ThemeButton() {
  const t = useT();
  const runtime = useStyleRuntime();
  const [mode, setMode] = useState<ThemeMode>('system');

  useEffect(() => {
    setMode(readMode());
  }, []);

  const pick = (next: ThemeMode) => {
    setMode(next);
    writeMode(next, runtime);
  };

  return (
    <Menu.Root label={t('appearance.title')} align="end">
      <Menu.Trigger icon={ICON[mode]} variant="ghost" diameter={40} radius="md" />
      {(['system', 'light', 'dark'] as const).map((value) => (
        <Menu.Item
          key={value}
          icon={ICON[value]}
          label={`${t(`appearance.${value}`)}${mode === value ? '  ✓' : ''}`}
          onSelect={() => pick(value)}
        />
      ))}
    </Menu.Root>
  );
}
