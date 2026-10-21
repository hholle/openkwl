import '@mantine/core/styles.css';
import '@mantine/dates/styles.css';
import './theme/mantine-overrides.css';

import 'dayjs/locale/de';

import { MantineProvider } from '@mantine/core';
import { DatesProvider } from '@mantine/dates';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { language, useLanguageKey } from './i18n';
import { resolver, theme } from './theme/mantine-theme';

/** Remounts the app when the language changes; date pickers follow it (Monday first). */
function LanguageRoot() {
  const key = useLanguageKey();
  return (
    <DatesProvider key={key} settings={{ locale: language(), firstDayOfWeek: 1 }}>
      <App />
    </DatesProvider>
  );
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <MantineProvider theme={theme} cssVariablesResolver={resolver} defaultColorScheme="auto">
      <LanguageRoot />
    </MantineProvider>
  </StrictMode>,
);
