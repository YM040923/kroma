import { useT } from '@kromatv/ui';
import { Box, Button, IconButton, ListRow, Spinner, Switch, Text } from '@kromatv/ui/kit';
import { useIsMutating, useMutation, useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { SettingCard } from '#web/features/accounts/account/setting-card.component';
import { StatusText } from '#web/features/accounts/account/ui';
import {
  DEFAULT_HOME_LAYOUT,
  type HomeLayout,
  layoutInventory,
  moveModule,
  orderedModules,
} from '#web/features/catalog/home/home-layout';
import { saveAccountHomeLayout } from '#web/features/catalog/home/home-layout-api';
import { useWebRuntime } from '#web/shared/lib/web-runtime';

type HomeModule = ReturnType<typeof layoutInventory>[number];

function sameLayout(a: HomeLayout, b: HomeLayout) {
  return (
    a.order.length === b.order.length &&
    a.order.every((id, i) => id === b.order[i]) &&
    a.hidden.length === b.hidden.length &&
    a.hidden.every((id) => b.hidden.includes(id))
  );
}

function moveRow(layout: HomeLayout, inventory: HomeModule[], id: string, offset: -1 | 1) {
  const ids = orderedModules(inventory, layout.order).map((row) => row.id);
  const order = moveModule(ids, id, offset);
  // Move between listed neighbours, leaving any absent server rows in their saved slots.
  const listed = new Set(ids);
  for (const [index, saved] of layout.order.entries()) {
    if (!listed.has(saved)) {
      order.splice(index, 0, saved);
    }
  }
  return { ...layout, order };
}

export function HomeLayoutCard({ viewer }: Readonly<{ viewer: string }>) {
  const t = useT();
  const runtime = useWebRuntime();
  const { userQueries } = runtime.queries;
  const layoutOptions = userQueries.homeLayout(viewer);
  const layout = useQuery(layoutOptions);
  const libraries = useQuery(runtime.queries.catalogQueries.libraries(viewer));
  const sections = useQuery(userQueries.home());
  const watching = useQuery(userQueries.continueWatching());
  const [draft, setDraft] = useState<HomeLayout | null>(null);
  const writes = { mutationKey: ['account', 'saveHomeLayout', viewer], exact: true };
  const saving = useIsMutating(writes) > 0;
  const save = useMutation({
    mutationKey: writes.mutationKey,
    networkMode: 'always',
    retry: false,
    mutationFn: (next: HomeLayout) => saveAccountHomeLayout(runtime, viewer, next),
  });
  const heading = {
    icon: 'home',
    title: t('account.homeModules'),
    description: t('account.homeModulesDesc'),
  } as const;

  const queries = [layout, sections, watching, libraries];
  if (
    !layout.isSuccess ||
    !sections.isSuccess ||
    !libraries.isSuccess ||
    !watching.isSuccess ||
    queries.some((query) => query.isPlaceholderData) ||
    (saving && !draft)
  ) {
    const failed = queries.filter((query) => query.isError);
    const retry = () => {
      for (const query of failed) {
        void query.refetch();
      }
    };
    return (
      <SettingCard {...heading}>
        {failed.length > 0 ? (
          <Box row wrap align="center" gap={12}>
            <Box role="alert">
              <Text variant="meta" color="danger">
                {t('account.homeLoadFailed')}
              </Text>
            </Box>
            <Button size="sm" variant="outline" label={t('error.retry')} onPress={retry} />
          </Box>
        ) : (
          <Spinner size={22} label={t('common.loading')} />
        )}
      </SettingCard>
    );
  }

  const stored = layout.data;
  const value = draft ?? stored;
  const dirty = !sameLayout(value, stored);
  const inventory = layoutInventory(sections.data, t, watching.data.length > 0, libraries.data);
  const rows = orderedModules(inventory, value.order);
  const edit = (change: (previous: HomeLayout) => HomeLayout) => {
    if (save.isError || save.isSuccess) {
      save.reset();
    }
    const confirmed = runtime.queryClient.getQueryData(layoutOptions.queryKey) ?? stored;
    const writing = runtime.queryClient.isMutating(writes) > 0;
    setDraft((previous) => {
      const next = change(previous ?? confirmed);
      return !writing && sameLayout(next, confirmed) ? null : next;
    });
  };
  const submit = () => {
    // The cache lock is synchronous, even before a second click sees the busy button.
    if (!draft || !dirty || runtime.queryClient.isMutating(writes)) {
      return;
    }
    save.mutate(draft, {
      // The writer updates this viewer's query. Don't discard edits made during that write.
      onSuccess: () => setDraft((current) => (current === draft ? null : current)),
    });
  };

  return (
    <SettingCard
      {...heading}
      footer={
        <>
          <Button
            size="sm"
            variant="outline"
            label={t('account.homeReset')}
            disabled={value.order.length === 0 && value.hidden.length === 0}
            onPress={() => edit(() => DEFAULT_HOME_LAYOUT)}
          />
          <Box row wrap align="center" gap={12}>
            <Box aria-live="polite">
              {dirty && !save.isError && !saving ? (
                <Text variant="meta" color="textDim">
                  {t('common.unsaved')}
                </Text>
              ) : (
                <StatusText
                  status={save.isError ? 'error' : save.isSuccess && !dirty ? 'saved' : 'idle'}
                  error={t('account.saveFailed')}
                />
              )}
            </Box>
            <Button
              size="sm"
              label={saving ? t('common.saving') : save.isError ? t('error.retry') : t('common.save')}
              loading={saving}
              disabled={!dirty}
              onPress={submit}
            />
          </Box>
        </>
      }
    >
      <ListRow.Group size="md">
        {rows.map(({ id, title, available }, index) => (
          <ListRow.Root key={id}>
            <ListRow.Label>{title}</ListRow.Label>
            {available ? null : <ListRow.Hint>{t('account.homeUnavailable')}</ListRow.Hint>}
            <ListRow.Trailing>
              <Box row gap={2}>
                <IconButton
                  control="md"
                  variant="ghost"
                  icon="arrow-up"
                  label={t('account.homeMoveUp', { title })}
                  disabled={index === 0}
                  onPress={() => edit((prev) => moveRow(prev, inventory, id, -1))}
                />
                <IconButton
                  control="md"
                  variant="ghost"
                  icon="arrow-down"
                  label={t('account.homeMoveDown', { title })}
                  disabled={index === rows.length - 1}
                  onPress={() => edit((prev) => moveRow(prev, inventory, id, 1))}
                />
              </Box>
              <Switch
                label={t('account.homeVisibility', { title })}
                checked={!value.hidden.includes(id)}
                onCheckedChange={(visible) =>
                  edit((prev) => {
                    const others = prev.hidden.filter((one) => one !== id);
                    return { ...prev, hidden: visible ? others : [...others, id] };
                  })
                }
              />
            </ListRow.Trailing>
          </ListRow.Root>
        ))}
      </ListRow.Group>
    </SettingCard>
  );
}
