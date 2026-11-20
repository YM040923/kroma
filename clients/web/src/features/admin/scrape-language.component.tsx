import { langName } from '@kromatv/core';
import { useT } from '@kromatv/ui';
import { Box, Chip, IconButton, Row, Surface, Text } from '@kromatv/ui/kit';

export const MAX_SCRAPE_LANGUAGES = 3;

/** Chips to add a language, then an ordered list: the first tag is what TMDB
 *  is asked in, the rest fill gaps. Values are TMDB tags ("zh-CN"). */
export function LanguagePriorityEditor({
  value,
  languages,
  min = 0,
  onChange,
}: Readonly<{
  value: string[];
  languages: string[];
  /** How many selections must remain; the last ones cannot be removed. */
  min?: number;
  onChange: (next: string[]) => void;
}>) {
  const t = useT();

  const toggle = (tag: string) => {
    if (value.includes(tag)) {
      if (value.length <= min) {
        return;
      }
      onChange(value.filter((held) => held !== tag));
      return;
    }
    if (value.length < MAX_SCRAPE_LANGUAGES) {
      onChange([...value, tag]);
    }
  };

  const move = (index: number, delta: number) => {
    const next = [...value];
    const [held] = next.splice(index, 1);
    if (held === undefined) {
      return;
    }
    next.splice(index + delta, 0, held);
    onChange(next);
  };

  return (
    <Box gap={12}>
      <Row wrap gap={8}>
        {languages.map((tag) => {
          const active = value.includes(tag);
          return (
            <Chip
              key={tag}
              variant="surface"
              active={active}
              label={langName(t, tag) ?? tag}
              disabled={
                (!active && value.length >= MAX_SCRAPE_LANGUAGES) ||
                (active && value.length <= min)
              }
              onPress={() => toggle(tag)}
            />
          );
        })}
      </Row>
      {value.length > 0 ? (
        <Row wrap gap={8}>
          {value.map((tag, index) => (
            <Surface
              key={tag}
              pad="none"
              radius="pill"
              border="border"
              row
              align="center"
              gap={4}
              px={12}
              py={4}
            >
              <Text variant="meta" color={index === 0 ? 'accentText' : 'textDim'}>
                {index === 0 ? t('admin.scrapePrimary') : String(index + 1)}
              </Text>
              <Text variant="label">{langName(t, tag) ?? tag}</Text>
              <IconButton
                variant="ghost"
                diameter={24}
                glyph={13}
                icon="arrow-up"
                label={t('admin.scrapeMoveUp')}
                disabled={index === 0}
                onPress={() => move(index, -1)}
              />
              <IconButton
                variant="ghost"
                diameter={24}
                glyph={13}
                icon="arrow-down"
                label={t('admin.scrapeMoveDown')}
                disabled={index === value.length - 1}
                onPress={() => move(index, 1)}
              />
              <IconButton
                variant="ghost"
                diameter={24}
                glyph={13}
                icon="x"
                label={t('admin.scrapeRemoveLanguage')}
                disabled={value.length <= min}
                onPress={() => toggle(tag)}
              />
            </Surface>
          ))}
        </Row>
      ) : null}
      <Text variant="meta" color="textDim">
        {t('admin.scrapeFallbackHint')}
      </Text>
    </Box>
  );
}