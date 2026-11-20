import { ItemId } from '@kromatv/client/media';
import type { ShareView } from '@kromatv/client/share';
import { useT } from '@kromatv/ui';
import { Box, Button, Chip, Field, Row, Text } from '@kromatv/ui/kit';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { useAuth } from '#web/shared/lib/auth';

const DAYS = [1, 3, 7, 30] as const;

/** Admin-only: mint or revoke the public link to one title. Shown under the
 *  fiche's management strip; a member never sees it. */
export function SharePanel({ id }: Readonly<{ id: string }>) {
  const t = useT();
  const { client, user } = useAuth();
  const queryClient = useQueryClient();
  const canManage = user?.permissions.includes('users.manage') ?? false;
  const item = ItemId.parse(id);
  const [days, setDays] = useState<number>(7);
  const [password, setPassword] = useState('');
  const [copied, setCopied] = useState(false);
  const [code, setCode] = useState<string | null>(null);

  const current = useQuery({
    queryKey: ['share', 'item', id],
    queryFn: () => client.share.current(item),
    enabled: canManage,
  });
  const refresh = () => queryClient.invalidateQueries({ queryKey: ['share', 'item', id] });

  const create = useMutation({
    mutationFn: () =>
      client.share.create(item, {
        days,
        password: password.trim() || null,
      }),
    onSuccess: (view) => {
      setPassword('');
      setCode(view.password ?? null);
      void refresh();
    },
  });
  const revoke = useMutation({
    mutationFn: () => client.share.revoke(item),
    onSuccess: () => void refresh(),
  });

  if (!canManage) {
    return null;
  }
  const share: ShareView | null | undefined = current.data;

  const copy = (view: ShareView) => {
    void navigator.clipboard
      .writeText(`${window.location.origin}${view.path}`)
      .then(() => {
        setCopied(true);
        setTimeout(() => setCopied(false), 1500);
      })
      .catch(() => undefined);
  };

  return (
    <section style={{ paddingLeft: 'var(--gutter-web)', paddingRight: 'var(--gutter-web)', marginTop: 36 }}>
      <Text variant="h2" mb={8}>
        {t('content.share')}
      </Text>
      <Text variant="meta" color="textDim" mb={14}>
        {t('content.shareHint')}
      </Text>
      {share ? (
        <Box gap={10}>
          <Row wrap gap={10} center>
            <Text variant="meta" color="textMuted">
              {`${window.location.origin}${share.path}`}
            </Text>
            <Button
              label={copied ? t('content.shareCopied') : t('content.shareCopy')}
              icon="copy"
              variant="glass"
              size="sm"
              onPress={() => copy(share)}
            />
          </Row>
          <Text variant="meta" color="textDim">
            {t('content.shareExpires', {
              time: new Date(share.expiresAt * 1000).toLocaleString(),
            })}
            {share.requiresPassword ? ` · ${t('content.shareHasPassword')}` : ''}
          </Text>
          {code ? (
            <Text variant="meta" color="textMuted">
              {t('content.shareCode', { code })}
            </Text>
          ) : null}
          <Row>
            <Button
              label={t('content.shareRevoke')}
              variant="dangerGhost"
              size="sm"
              onPress={() => revoke.mutate()}
              disabled={revoke.isPending}
            />
          </Row>
        </Box>
      ) : (
        <Box gap={12}>
          <Row wrap gap={8}>
            {DAYS.map((value) => (
              <Chip
                key={value}
                active={days === value}
                variant="subtle"
                label={t('content.shareDays', { days: value })}
                onPress={() => setDays(value)}
              />
            ))}
          </Row>
          <div style={{ maxWidth: 320 }}>
            <Field.Root
              label={t('content.sharePasswordLabel')}
              value={password}
              onValueChange={setPassword}
            >
              <Field.Input type="password" placeholder={t('content.sharePasswordHint')} />
            </Field.Root>
          </div>
          <Row>
            <Button
              label={t('content.shareGenerate')}
              icon="link"
              variant="primary"
              size="sm"
              onPress={() => create.mutate()}
              disabled={create.isPending}
            />
          </Row>
        </Box>
      )}
      {create.isError || revoke.isError ? (
        <Text variant="meta" color="danger" mt={8}>
          {t('admin.updateFailed')}
        </Text>
      ) : null}
    </section>
  );
}
