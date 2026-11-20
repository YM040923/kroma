import { useT } from '@kromatv/ui';
import { Box, Button, Field, Text } from '@kromatv/ui/kit';
import { useMutation, useQuery } from '@tanstack/react-query';
import { createFileRoute } from '@tanstack/react-router';
import { useState } from 'react';
import { imageUrl } from '#web/shared/lib/api';
import { useAuth } from '#web/shared/lib/auth';

// A public link to one title: no navigation, no library, no account. The
// password, when there is one, is asked before anything about the film shows.
export const Route = createFileRoute('/s/$slug')({
  component: SharePage,
});

const PAGE: React.CSSProperties = {
  minHeight: '100vh',
  background: 'var(--kroma-bg)',
  color: 'var(--kroma-text)',
  display: 'flex',
  justifyContent: 'center',
  padding: '48px 20px 80px',
};

const SHELL: React.CSSProperties = {
  width: 'min(960px, 100%)',
  display: 'flex',
  flexDirection: 'column',
  gap: 22,
};

function SharePage() {
  const t = useT();
  const slug = Route.useParams().slug;
  const { client } = useAuth();
  const [password, setPassword] = useState('');

  const info = useQuery({
    queryKey: ['share', slug],
    queryFn: () => client.share.info(slug),
    retry: 0,
  });
  const unlock = useMutation({
    mutationFn: () => client.share.unlock(slug, password),
    onSuccess: () => void info.refetch(),
  });

  if (info.isError || (info.data && !info.data.unlocked && !info.data.requiresPassword)) {
    return (
      <div style={PAGE}>
        <div style={SHELL}>
          <Text variant="h2">{t('content.shareGone')}</Text>
        </div>
      </div>
    );
  }
  if (info.isPending) {
    return (
      <div style={PAGE}>
        <div style={SHELL}>
          <Text variant="meta" color="textDim">
            …
          </Text>
        </div>
      </div>
    );
  }
  const share = info.data;
  if (!share.unlocked) {
    return (
      <div style={PAGE}>
        <div style={{ ...SHELL, maxWidth: 420 }}>
          <Text variant="h2">{t('content.shareProtected')}</Text>
          <Text variant="meta" color="textDim">
            {t('content.sharePageHint')}
          </Text>
          <Field.Root label={t('content.sharePasswordLabel')} value={password} onValueChange={setPassword}>
            <Field.Input type="password" />
          </Field.Root>
          <Button
            label={t('content.shareUnlock')}
            variant="primary"
            onPress={() => unlock.mutate()}
            disabled={unlock.isPending || password.trim().length === 0}
          />
          {unlock.isError ? (
            <Text variant="meta" color="danger">
              {t('content.shareWrong')}
            </Text>
          ) : null}
        </div>
      </div>
    );
  }

  const poster = imageUrl(share.posterUrl);
  return (
    <div style={PAGE}>
      <div style={SHELL}>
        <Box row gap={20} align="flex-start" wrap>
          {poster ? (
            <img
              src={poster}
              alt={share.title ?? ''}
              style={{ width: 180, borderRadius: 14, display: 'block' }}
            />
          ) : null}
          <Box shrink={1} minW={0} gap={8}>
            <Text variant="h1">{share.title}</Text>
            <Text variant="meta" color="textDim">
              {[share.year, new Date(share.expiresAt * 1000).toLocaleDateString()]
                .filter(Boolean)
                .join(' · ')}
            </Text>
            {share.overview ? (
              <Text variant="body" color="textMuted" mt={8}>
                {share.overview}
              </Text>
            ) : null}
          </Box>
        </Box>
        {share.hasFile ? (
          <video
            controls
            playsInline
            autoPlay
            src={client.share.streamUrl(slug)}
            style={{ width: '100%', borderRadius: 14, background: '#000' }}
          />
        ) : (
          <Text variant="meta" color="textDim">
            {t('content.shareNoFile')}
          </Text>
        )}
      </div>
    </div>
  );
}
