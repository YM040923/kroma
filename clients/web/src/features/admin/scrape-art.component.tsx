import { langName } from '@kromatv/core';
import { useT } from '@kromatv/ui';
import { Box, Button, Chip, Field, Row, Text } from '@kromatv/ui/kit';
import { LanguagePriorityEditor } from '#web/features/admin/scrape-language.component';

export const ART_SIZES = ['original', 'w1280', 'w780', 'w500'] as const;

export interface ArtRuleValue {
  languages: string[] | null;
  minWidth: number | null;
  size: string | null;
}

/** One artwork rule: which languages are tried in order, the smallest
 *  candidate accepted, and the provider size stored. */
export function ArtRuleEditor({
  title,
  hint,
  value,
  languages,
  minWidthPlaceholder = '500',
  sources,
  overridden,
  onFollow,
  onChange,
}: Readonly<{
  title: string;
  hint: string;
  value: ArtRuleValue;
  languages: string[];
  minWidthPlaceholder?: string;
  /** Provider order; shown only when a second source is configured. */
  sources?: { value: string[]; onChange: (next: string[]) => void };
  /** Undefined on the global page; false/true states the library override. */
  overridden?: boolean;
  onFollow?: () => void;
  onChange: (patch: Partial<ArtRuleValue>) => void;
}>) {
  const t = useT();
  const label = (tag: string) =>
    tag === 'textless' ? t('admin.artTextless') : (langName(t, tag) ?? tag);

  return (
    <Box gap={12}>
      <Row between gap={12} wrap>
        <Box>
          <Text variant="label">{title}</Text>
          <Text variant="meta" color="textDim" mt={2}>
            {hint}
          </Text>
        </Box>
        {overridden === undefined ? null : overridden ? (
          <Button
            variant="ghost"
            size="sm"
            label={t('admin.followAction')}
            onPress={onFollow}
          />
        ) : (
          <Text variant="meta" color="textDim">
            {t('admin.followGlobal')}
          </Text>
        )}
      </Row>
      <LanguagePriorityEditor
        value={value.languages ?? []}
        languages={[...languages, 'textless']}
        max={4}
        labelOf={label}
        onChange={(next) => onChange({ languages: next.length > 0 ? next : null })}
      />
      {sources && sources.value.length > 0 ? (
        <Row wrap gap={8} align="center">
          <Text variant="meta" color="textDim">
            {t('admin.sourcesOrder')}
          </Text>
          {sources.value.map((source, index) => (
            <Chip
              key={source}
              variant="surface"
              label={`${index + 1} ${source === 'tmdb' ? 'TMDB' : 'Fanart'}`}
            />
          ))}
          <Button
            variant="ghost"
            size="sm"
            label={t('admin.swapSources')}
            onPress={() => sources.onChange([...sources.value].reverse())}
          />
        </Row>
      ) : null}
      <Row wrap gap={20} align="flex-end">
        <Box w={150}>
          <Field.Root
            label={t('admin.artMinWidth')}
            value={value.minWidth == null ? '' : String(value.minWidth)}
            onValueChange={(text) => {
              const digits = text.replace(/[^0-9]/g, '');
              onChange({ minWidth: digits ? Number.parseInt(digits, 10) : null });
            }}
          >
            <Field.Input placeholder={minWidthPlaceholder} />
          </Field.Root>
        </Box>
        <Box>
          <Text variant="meta" color="textDim" mb={6}>
            {t('admin.artSize')}
          </Text>
          <Row wrap gap={8}>
            <Chip
              variant="surface"
              active={value.size == null}
              label={t('admin.artSizeAuto')}
              onPress={() => onChange({ size: null })}
            />
            {ART_SIZES.map((size) => (
              <Chip
                key={size}
                variant="surface"
                active={value.size === size}
                label={
                  size === 'original' ? t('admin.artSizeOriginal') : size.replace('w', '')
                }
                onPress={() => onChange({ size })}
              />
            ))}
          </Row>
        </Box>
      </Row>
    </Box>
  );
}