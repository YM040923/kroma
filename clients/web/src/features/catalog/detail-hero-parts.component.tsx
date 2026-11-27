import type { CrewMember } from '@kromatv/client/media';
import { personSegment } from '@kromatv/core';
import { useT } from '@kromatv/ui';
import {
  Box,
  Button,
  CONTROL,
  DataField,
  IconButton,
  styles,
  useEntryDefaults,
  useStyles,
} from '@kromatv/ui/kit';
import { Link } from '@tanstack/react-router';
import { useFocusRing } from '#web/shared/lib/use-focus-ring.hook';

const FIELD_GAP_X = { base: 20, md: 32 } as const;
const sDefinition = styles({
  link: { color: 'inherit', textDecorationLine: 'underline', textUnderlineOffset: 2, radius: 'xs' },
});

function DirectorLink({ person, label }: Readonly<{ person: CrewMember; label: string }>) {
  const s = useStyles(sDefinition);

  const focus = useFocusRing(s.link);
  return (
    <Link
      to="/people/$person"
      params={{ person: personSegment(person) }}
      aria-label={label}
      className={focus.className}
      {...focus.bind}
    >
      {person.name}
    </Link>
  );
}

function DirectorsField({ directors }: Readonly<{ directors: CrewMember[] }>) {
  const t = useT();
  return (
    <DataField.Root size="sm">
      <DataField.Label>{t('content.directedBy')}</DataField.Label>
      <DataField.Value>
        {directors.map((d, i) => (
          <span key={personSegment(d)}>
            {i > 0 ? ', ' : ''}
            <DirectorLink person={d} label={t('person.viewWorks', { name: d.name })} />
          </span>
        ))}
      </DataField.Value>
    </DataField.Root>
  );
}

export function WatchedButton({
  watched,
  onToggle,
}: Readonly<{ watched?: boolean; onToggle?: () => void }>) {
  const t = useT();
  if (!onToggle) {
    return null;
  }
  return (
    <Button
      variant="outline"
      active={watched ?? false}
      pressed={watched ?? false}
      icon="check"
      label={watched ? t('content.watched') : t('content.markWatched')}
      onPress={onToggle}
    />
  );
}

export function ListButton({
  inList,
  onToggle,
}: Readonly<{ inList?: boolean; onToggle?: () => void }>) {
  const entryDefaults = useEntryDefaults();
  const t = useT();
  if (!onToggle) {
    return null;
  }
  return (
    <IconButton
      diameter={CONTROL[entryDefaults.size].height}
      glyph={20}
      radius="md"
      active={inList ?? false}
      pressed={inList ?? false}
      icon={inList ? 'bookmark-filled' : 'bookmark'}
      label={inList ? t('content.removeFromList') : t('content.addToList')}
      onPress={onToggle}
    />
  );
}

export function ReportButton({ onReport }: Readonly<{ onReport?: () => void }>) {
  const entryDefaults = useEntryDefaults();
  const t = useT();
  if (!onReport) {
    return null;
  }
  return (
    <IconButton
      diameter={CONTROL[entryDefaults.size].height}
      glyph={19}
      radius="md"
      icon="flag"
      label={t('report.action')}
      onPress={onReport}
    />
  );
}

export function HeroFields({ directors, audio, subtitles }: Readonly<{
  directors?: CrewMember[];
  audio?: string;
  subtitles?: string;
}>) {
  const t = useT();
  if (!directors?.length && audio == null && subtitles == null) return null;
  return (
    <Box row wrap gapX={FIELD_GAP_X} gapY={12} mt={16}>
      {directors?.length ? <DirectorsField directors={directors} /> : null}
      {audio != null ? (
        <DataField.Root size="sm">
          <DataField.Label>{t('content.fieldAudio')}</DataField.Label>
          <DataField.Value>{audio}</DataField.Value>
        </DataField.Root>
      ) : null}
      {subtitles != null ? (
        <DataField.Root size="sm">
          <DataField.Label>{t('content.fieldSubtitles')}</DataField.Label>
          <DataField.Value>{subtitles}</DataField.Value>
        </DataField.Root>
      ) : null}
    </Box>
  );
}
