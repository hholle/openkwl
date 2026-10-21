import { Box, Group, Stack } from '@mantine/core';
import type { ReactNode } from 'react';
import { Wordmark } from './Wordmark';

/** Login and setup: the wordmark centred above one narrow column on the app background. */
export function Centered({ width, children }: { width: number; children: ReactNode }) {
  return (
    <Box className="app-main" mih="100vh" px="lg" py={{ base: 'xl', sm: '10vh' }}>
      <Stack maw={width} mx="auto" gap="lg">
        <Group justify="center">
          <Wordmark height={24} />
        </Group>
        {children}
      </Stack>
    </Box>
  );
}
