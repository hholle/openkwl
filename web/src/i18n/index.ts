// Languages of the web UI with i18next: English and German, following the browser unless chosen
// in Settings › Language (remembered per browser). The copy lives in `locales/<lang>/<area>.json`
// (German must have every English key, `locales.test.ts`); numbers, dates and times are
// formatted by `Intl` for the browser's region (`format.ts`).
//
// In components: `const { t } = useTranslation('settings')`, then `t('filters.title')`.
// Outside components (summaries, sentences): `t('overview:next', { … })` from this module.
// A language change remounts the app (`useLanguageKey`), so everything reads the new one.

import i18next from 'i18next';
import { useEffect, useState } from 'react';
import { initReactI18next } from 'react-i18next';
import deApp from './locales/de/app.json';
import deCommon from './locales/de/common.json';
import deDetails from './locales/de/details.json';
import deFaults from './locales/de/faults.json';
import deOverview from './locales/de/overview.json';
import deSchedule from './locales/de/schedule.json';
import deSettings from './locales/de/settings.json';
import deSetup from './locales/de/setup.json';
import deUpdate from './locales/de/update.json';
import enApp from './locales/en/app.json';
import enCommon from './locales/en/common.json';
import enDetails from './locales/en/details.json';
import enFaults from './locales/en/faults.json';
import enOverview from './locales/en/overview.json';
import enSchedule from './locales/en/schedule.json';
import enSettings from './locales/en/settings.json';
import enSetup from './locales/en/setup.json';
import enUpdate from './locales/en/update.json';

/** English is the reference; its files define the keys (and their types, `i18next.d.ts`). */
export const resources = {
  en: { common: enCommon, app: enApp, overview: enOverview, schedule: enSchedule, settings: enSettings, details: enDetails, faults: enFaults, setup: enSetup, update: enUpdate },
  de: { common: deCommon, app: deApp, overview: deOverview, schedule: deSchedule, settings: deSettings, details: deDetails, faults: deFaults, setup: deSetup, update: deUpdate },
} as const;

export type Lang = keyof typeof resources;
/** What the user picked: a language, or the browser's. */
export type LangChoice = Lang | 'auto';

const KEY = 'openkwl.lang';
/** The region used when the browser's own doesn't match the chosen language. */
const DEFAULT_LOCALE: Record<Lang, string> = { en: 'en-GB', de: 'de-DE' };

function browserLocales(): readonly string[] {
  if (typeof navigator === 'undefined') return [];
  return navigator.languages?.length ? navigator.languages : [navigator.language];
}

function readChoice(): LangChoice {
  try {
    const v = localStorage.getItem(KEY);
    return v === 'en' || v === 'de' ? v : 'auto';
  } catch {
    return 'auto';
  }
}

/** The language and the `Intl` locale for a choice: the first browser language we have, and
 *  the browser's region for it (`de-CH`, `en-US`), else a default region. */
export function resolve(choice: LangChoice, browser: readonly string[] = browserLocales()): { lang: Lang; locale: string } {
  const ours = (l: string) => l.slice(0, 2).toLowerCase() as Lang;
  const lang = choice !== 'auto' ? choice : (browser.map(ours).find((l) => l in resources) ?? 'en');
  const locale = browser.find((l) => ours(l) === lang && l.length > 2) ?? DEFAULT_LOCALE[lang];
  return { lang, locale };
}

let choice: LangChoice = readChoice();
let current = resolve(choice);

/** The current `Intl` locale, e.g. `de-DE`, `en-US` (live binding). */
export let locale: string = current.locale;
export const language = (): Lang => current.lang;
export const languageChoice = (): LangChoice => choice;

void i18next.use(initReactI18next).init({
  resources,
  lng: current.lang,
  fallbackLng: 'en',
  ns: Object.keys(resources.en),
  defaultNS: 'common',
  // Everything is bundled: ready before the first render.
  initAsync: false,
  // React escapes already.
  interpolation: { escapeValue: false },
  returnNull: false,
});
if (typeof document !== 'undefined') document.documentElement.lang = current.lang;

/** Translation outside components (same keys as `useTranslation`). */
export const t = i18next.t.bind(i18next) as typeof i18next.t;

const listeners = new Set<() => void>();

export function setLanguage(next: LangChoice) {
  choice = next;
  try {
    if (next === 'auto') localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, next);
  } catch {
    // Private window: the choice lasts until the page closes.
  }
  current = resolve(choice);
  locale = current.locale;
  if (typeof document !== 'undefined') document.documentElement.lang = current.lang;
  void i18next.changeLanguage(current.lang);
  listeners.forEach((l) => l());
}

/** Changes with the language; used as a key so everything below remounts with it. */
export function useLanguageKey(): string {
  const [key, setKey] = useState(current.locale + choice);
  useEffect(() => {
    const l = () => setKey(current.locale + choice);
    listeners.add(l);
    return () => {
      listeners.delete(l);
    };
  }, []);
  return key;
}
