import { langName } from '@kromatv/core';
import { useT } from '@kromatv/ui';
import { Box, Button, Chip, Row, Text } from '@kromatv/ui/kit';
import { LanguagePriorityEditor } from '#web/features/admin/scrape-language.component';

export const ART_SIZES = ['original', 'w1280', 'w780', 'w500'] as const;

export interface ArtRuleValue {
  languages: string[] | null;
  minWidth: number | null;
  minVotes: number | null;
  size: string | null;
}

/** One artwork rule: which languages are tried in order, the smallest
 *  candidate accepted, and the provider size stored. */
export function ArtRuleEditor({
  title,
  hint,
  value,
  languages,
  widthOptions = [500, 780, 1000, 1280, 1920],
  voteOptions,
  sources,
  overridden,
  onFollow,
  onChange,
}: Readonly<{
  title: string;
  hint: string;
  value: ArtRuleValue;
  languages: string[];
  widthOptions?: readonly number[];
  /** Favourites-count presets; absent hides the filter for this image type. */
  voteOptions?: readonly number[];
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
        <Box>
          <Text variant="meta" color="textDim" mb={6}>
            {t('admin.artMinWidth')}
          </Text>
          <Row wrap gap={8}>
            <Chip
              variant="surface"
              active={value.minWidth == null}
              label={t('admin.artWidthDefault')}
              onPress={() => onChange({ minWidth: null })}
            />
            {widthOptions.map((width) => (
              <Chip
                key={width}
                variant="surface"
                active={value.minWidth === width}
                label={String(width)}
                onPress={() => onChange({ minWidth: width })}
              />
            ))}
            {value.minWidth != null && !widthOptions.includes(value.minWidth) ? (
              <Chip
                variant="surface"
                active
                label={String(value.minWidth)}
                onPress={() => onChange({ minWidth: null })}
              />
            ) : null}
          </Row>
        </Box>
        {voteOptions ? (
          <Box>
            <Text variant="meta" color="textDim" mb={6}>
              {t('admin.artMinVotes')}
            </Text>
            <Row wrap gap={8}>
              <Chip
                variant="surface"
                active={value.minVotes == null}
                label={t('admin.artVotesAny')}
                onPress={() => onChange({ minVotes: null })}
              />
              {voteOptions.map((votes) => (
                <Chip
                  key={votes}
                  variant="surface"
                  active={value.minVotes === votes}
                  label={String(votes)}
                  onPress={() => onChange({ minVotes: votes })}
                />
              ))}
            </Row>
          </Box>
        ) : null}
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