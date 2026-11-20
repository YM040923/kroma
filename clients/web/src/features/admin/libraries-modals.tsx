import type { MessageKey } from '@kromatv/i18n';
import { useT } from '@kromatv/ui';
import {
  Box,
  Button,
  Dialog,
  Field,
  Grid,
  Icon,
  type IconName,
  Row,
  Surface,
  Text,
} from '@kromatv/ui/kit';
import type { CSSProperties } from 'react';
import { useState } from 'react';
import { createCallable } from 'react-call';
import { FolderListEditor } from '#web/shared/ui/folder-field.component';
import { useAsyncAction } from '#web/features/admin/shell';
import { useAuth } from '#web/shared/lib/auth';

/** Library kind as accepted by the create/update API: `""` = Auto (legacy). */
export type LibKind = '' | 'movies' | 'shows' | 'mixed' | 'photos' | 'other';

/** Accepts both the legacy `film`/`tv` kinds and the current `movies`/`shows`. */
export function normalizeLibKind(kind: string): LibKind {
  if (kind === 'shows' || kind === 'tv') {
    return 'shows';
  }
  if (kind === 'movies' || kind === 'film') {
    return 'movies';
  }
  if (kind === 'photos' || kind === 'photo') {
    return 'photos';
  }
  if (kind === 'other') {
    return 'other';
  }
  if (kind === 'mixed') {
    return 'mixed';
  }
  return '';
}

type CreateKind = Exclude<LibKind, ''>;

const CREATE_KINDS: readonly {
  value: CreateKind;
  icon: IconName;
  labelKey: MessageKey;
  descKey: MessageKey;
}[] = [
  { value: 'movies', icon: 'movie', labelKey: 'admin.typeMovies', descKey: 'admin.typeMoviesDesc' },
  { value: 'shows', icon: 'device-tv', labelKey: 'admin.typeShows', descKey: 'admin.typeShowsDesc' },
  { value: 'mixed', icon: 'apps', labelKey: 'admin.typeMixed', descKey: 'admin.typeMixedDesc' },
  { value: 'photos', icon: 'photo', labelKey: 'admin.typePhotos', descKey: 'admin.typePhotosDesc' },
  { value: 'other', icon: 'folder', labelKey: 'admin.typeOther', descKey: 'admin.typeOtherDesc' },
];

const cardStyle = (active: boolean): CSSProperties => ({
  display: 'flex',
  flexDirection: 'column',
  alignItems: 'flex-start',
  gap: 8,
  padding: 16,
  minHeight: 132,
  borderRadius: 16,
  border: `1px solid ${active ? 'var(--kroma-accent)' : 'var(--kroma-border)'}`,
  background: active
    ? 'color-mix(in srgb, var(--kroma-accent) 8%, var(--kroma-surface-1))'
    : 'var(--kroma-surface-1)',
  color: 'var(--kroma-text)',
  textAlign: 'left',
  cursor: 'pointer',
});

export const AddLibraryModal = createCallable<void, boolean>(({ call }) => {
  const t = useT();
  const { client } = useAuth();
  const [step, setStep] = useState<1 | 2>(1);
  const [kind, setKind] = useState<CreateKind | ''>('');
  const [name, setName] = useState('');
  const [folders, setFolders] = useState<string[]>([]);
  const { busy, run } = useAsyncAction();

  const create = () => {
    if (!name.trim() || !kind) {
      return;
    }
    void run(async () => {
      await client.library.create({
        name: name.trim(),
        kind,
        folders: folders.map((folder) => folder.trim()).filter(Boolean),
      });
      call.end(true);
    });
  };

  return (
    <Dialog.Root
      open
      title={t('admin.addLibrary')}
      width="lg"
      onClose={() => call.end(false)}
    >
      <Text variant="meta" color="textDim" mb={16}>
        {t('admin.wizardStep', { step, total: 2 })}
      </Text>
      {step === 1 ? (
        <Box gap={14}>
          <Text variant="label">{t('admin.wizardPickType')}</Text>
          <Grid min={230} gap={12}>
            {CREATE_KINDS.map((option) => {
              const active = kind === option.value;
              return (
                <div
                  key={option.value}
                  role="button"
                  tabIndex={0}
                  aria-pressed={active}
                  style={cardStyle(active)}
                  onClick={() => setKind(option.value)}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter' || event.key === ' ') {
                      event.preventDefault();
                      setKind(option.value);
                    }
                  }}
                >
                  <Row between w="100%">
                    <Icon name={option.icon} size={26} thickness={1.7} color="accentText" />
                    {active ? <Icon name="circle-check" size={20} color="accentText" /> : null}
                  </Row>
                  <Text variant="title">{t(option.labelKey)}</Text>
                  <Text variant="meta" color="textDim">
                    {t(option.descKey)}
                  </Text>
                </div>
              );
            })}
          </Grid>
          <Text variant="meta" color="textDim">
            {t('admin.typeLocked')}
          </Text>
        </Box>
      ) : (
        <Box gap={18}>
          <Row gap={10} align="center">
            <Text variant="label">{t(CREATE_KINDS.find((k) => k.value === kind)?.labelKey ?? 'admin.typeMovies')}</Text>
            <Button
              variant="ghost"
              size="sm"
              label={t('admin.wizardChangeType')}
              onPress={() => setStep(1)}
            />
          </Row>
          <Field.Root label={t('admin.name')}>
            <Field.Input
              icon="tag"
              value={name}
              onValueChange={setName}
              placeholder={t('admin.kindMovies')}
            />
          </Field.Root>
          <Field.Root label={t('admin.scannedFolders')}>
            <FolderListEditor folders={folders} onChange={setFolders} />
          </Field.Root>
          <Text variant="meta" color="textDim" mt={-8}>
            {t('admin.firstRootHint')}
          </Text>
          <Surface pad="none" radius="lg" border="border" px={16} py={14}>
            <Row gap={10} align="flex-start">
              <Icon name="info-circle" size={18} thickness={1.8} color="accentText" />
              <Text variant="meta" color="textMuted" flex>
                {t('admin.recommendedBox')}
              </Text>
            </Row>
          </Surface>
        </Box>
      )}
      <Dialog.Footer>
        <Row between gap={10}>
          <Button
            variant="ghost"
            label={t('common.cancel')}
            onPress={() => call.end(false)}
          />
          <Row gap={10}>
            {step === 2 ? (
              <Button
                variant="glass"
                label={t('admin.wizardBack')}
                onPress={() => setStep(1)}
              />
            ) : null}
            {step === 1 ? (
              <Button
                variant="primary"
                label={t('admin.nextName')}
                disabled={!kind}
                onPress={() => setStep(2)}
              />
            ) : (
              <Button
                variant="primary"
                label={busy ? t('common.creating') : t('admin.createScan')}
                loading={busy}
                disabled={!name.trim() || folders.length === 0}
                onPress={create}
              />
            )}
          </Row>
        </Row>
      </Dialog.Footer>
    </Dialog.Root>
  );
});