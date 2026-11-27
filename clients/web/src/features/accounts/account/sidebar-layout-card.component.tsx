import { useT } from '@kromatv/ui';
import { Box, Button, ListRow, Spinner, Switch, Text } from '@kromatv/ui/kit';
import { useIsMutating, useMutation } from '@tanstack/react-query';
import { useState } from 'react';
import { useSidebarInventory } from '#web/features/catalog/use-sidebar-inventory.hook';
import { useWebRuntime } from '#web/shared/lib/web-runtime';
import { SettingCard } from './setting-card.component';
import { StatusText } from './ui';

export function SidebarLayoutCard({ viewer }: Readonly<{ viewer: string }>) {
  const t = useT();
  const runtime = useWebRuntime();
  const { groups, layout, libraries } = useSidebarInventory();
  const [draft, setDraft] = useState<string[] | null>(null);
  const key = runtime.queries.userQueries.sidebarLayout(viewer).queryKey;
  const writes = { mutationKey: ['save-sidebar', runtime.client.baseUrl, viewer], exact: true };
  const busy = useIsMutating(writes) > 0;
  const save = useMutation({
    mutationKey: writes.mutationKey,
    retry: false,
    networkMode: 'always',
    mutationFn: async (hidden: string[]) => {
      const cancel = runtime.queryClient.cancelQueries({ queryKey: key, exact: true });
      const [saved] = await Promise.all([runtime.client.accounts.setSidebarLayout({ hidden }), cancel]);
      runtime.queryClient.setQueryData(key, saved);
    },
    onSuccess: () => setDraft(null),
  });
  const header = { icon: 'adjustments-horizontal', title: t('account.sidebar'), description: t('account.sidebarDesc') } as const;
  if (!layout.isSuccess || !libraries.isSuccess) {
    const failed = layout.isError || libraries.isError;
    return <SettingCard {...header}>{failed ? <Box gap={8} align="flex-start">
      <Box role="alert"><Text variant="meta" color="danger">{t('account.sidebarLoadFailed')}</Text></Box>
      <Button variant="outline" size="sm" label={t('error.retry')} onPress={() => {
        if (layout.isError) void layout.refetch();
        if (libraries.isError) void libraries.refetch();
      }} />
    </Box> : <Spinner size={22} label={t('common.loading')} />}</SettingCard>;
  }
  const hidden = new Set(draft ?? layout.data.hidden);
  const dirty = hidden.size !== layout.data.hidden.length || layout.data.hidden.some(id => !hidden.has(id));
  const toggle = (id: string, visible: boolean) => {
    save.reset();
    setDraft(prev => {
      const next = new Set(prev ?? layout.data.hidden);
      if (visible) next.delete(id); else next.add(id);
      return [...next];
    });
  };
  return <SettingCard {...header} footer={<>
    <Button size="sm" variant="outline" label={t('account.homeReset')} disabled={!hidden.size || busy} onPress={() => { save.reset(); setDraft([]); }} />
    <Box row wrap align="center" gap={12}>
      <Box aria-live="polite">{dirty && !busy && !save.isError ? <Text variant="meta" color="textDim">{t('common.unsaved')}</Text> :
        <StatusText status={save.isError ? 'error' : save.isSuccess && !dirty ? 'saved' : 'idle'} error={t('account.saveFailed')} />}</Box>
      <Button size="sm" label={t(busy ? 'common.saving' : save.isError ? 'error.retry' : 'common.save')} disabled={!dirty || busy} loading={busy} onPress={() => {
        if (draft && dirty && !runtime.queryClient.isMutating(writes)) save.mutate(draft);
      }} />
    </Box>
  </>}>
    {groups.filter(group => group.items.length).map(group => <Box key={group.id} gap={6}>
      <Text variant="overline" color="textMuted">{group.label}</Text>
      <ListRow.Group size="md">{group.items.map(item => <ListRow.Root key={item.id}>
        <ListRow.Label>{item.title}</ListRow.Label>
        <ListRow.Trailing><Switch label={t('account.sidebarVisibility', { title: item.title })} checked={!hidden.has(item.id)} disabled={busy} onCheckedChange={on => toggle(item.id, on)} /></ListRow.Trailing>
      </ListRow.Root>)}</ListRow.Group>
    </Box>)}
  </SettingCard>;
}
