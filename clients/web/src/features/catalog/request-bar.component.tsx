import { useT } from '@kromatv/ui';
import {
  Button,
  classes,
  IconButton,
  Row,
  Surface,
  styles,
  Text,
  useConfirm,
  useStyles,
} from '@kromatv/ui/kit';

const sDefinition = styles({
  sticky: {
    position: 'sticky',
    bottom: 16,
    zIndex: 3,
    paddingLeft: 'var(--gutter-web)',
    paddingRight: 'var(--gutter-web)',
    mt: 20,
  },
});

export function RequestBar({
  count,
  busy,
  onRequestSelected,
  onClear,
}: Readonly<{
  count: number;
  busy: boolean;
  onRequestSelected: () => void;
  onClear: () => void;
}>) {
  const s = useStyles(sDefinition);

  const t = useT();
  if (count === 0) {
    return null;
  }
  return (
    <div className={classes(s.sticky)}>
      <Surface elevated radius="xl" pad="md">
        <Row wrap between gap={12}>
          <Text variant="meta">{t('requests.selectedEpisodes', { count })}</Text>
          <Row wrap gap={8}>
            <Button
              size="sm"
              icon="plus"
              label={t('requests.askForSelected', { count })}
              onPress={onRequestSelected}
              loading={busy}
            />
            <IconButton
              variant="ghost"
              control="sm"
              icon="x"
              label={t('common.clear')}
              onPress={onClear}
            />
          </Row>
        </Row>
      </Surface>
    </div>
  );
}

export interface MissingSeason {
  number: number;
  count: number;
}

/** The request actions for what the season view is missing. Both submit a
 * request and say so; each asks for confirmation first, naming the seasons and
 * episodes it covers and warning that what is owned or already asked for is
 * skipped. A fully available current season never offers its own whole-season
 * request. */
export function SeasonAsk({
  season,
  seasonMissing,
  seasonRequestable,
  allMissing,
  busy,
  onRequestSeason,
  onRequestAll,
}: Readonly<{
  season: number;
  seasonMissing: number;
  seasonRequestable: boolean;
  allMissing: MissingSeason[];
  busy: boolean;
  onRequestSeason: () => void;
  onRequestAll: () => void;
}>) {
  const t = useT();
  const confirm = useConfirm();
  if (!seasonRequestable && allMissing.length === 0) {
    return null;
  }
  const askSeason = async () => {
    const ok = await confirm({
      title: t('requests.requestMissingTitle'),
      message: `${t('requests.requestSeasonScope', {
        number: season,
        count: seasonMissing,
      })} ${t('requests.requestSkipNote')}`,
      confirmLabel: t('requests.confirmRequest'),
      cancelLabel: t('common.cancel'),
    });
    if (ok) {
      onRequestSeason();
    }
  };
  const askAll = async () => {
    const scope = allMissing
      .map((one) =>
        t('requests.requestSeasonScopeLine', { number: one.number, count: one.count }),
      )
      .join(' · ');
    const ok = await confirm({
      title: t('requests.requestMissingTitle'),
      message: `${t('requests.requestAllScope', { seasons: scope })} ${t(
        'requests.requestSkipNote',
      )}`,
      confirmLabel: t('requests.confirmRequest'),
      cancelLabel: t('common.cancel'),
    });
    if (ok) {
      onRequestAll();
    }
  };
  return (
    <Row wrap gap={8} align="center">
      {seasonRequestable ? (
        <Button
          size="sm"
          icon="plus"
          label={t('requests.requestSeasonLabel', { number: season, count: seasonMissing })}
          onPress={() => void askSeason()}
          disabled={busy}
        />
      ) : null}
      {allMissing.length > 0 ? (
        <Button
          variant="outline"
          size="sm"
          icon="download"
          label={t('requests.requestAllMissingLabel', { count: allMissing.length })}
          onPress={() => void askAll()}
          disabled={busy}
        />
      ) : null}
    </Row>
  );
}
