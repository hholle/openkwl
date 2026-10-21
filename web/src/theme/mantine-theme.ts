// openKWL — Mantine theme for the firmware web UI (type-checked against Mantine 7.17 and 9.6)
// Usage:
//   import '@mantine/core/styles.css';
//   import './mantine-overrides.css';
//   <MantineProvider theme={theme} cssVariablesResolver={resolver} defaultColorScheme="light">
import {
  Alert,
  Badge,
  Button,
  Card,
  createTheme,
  CSSVariablesResolver,
  defaultVariantColorsResolver,
  Input,
  MantineColorsTuple,
  Modal,
  NumberInput,
  Paper,
  rem,
  SegmentedControl,
  Table,
  Title,
  VariantColorsResolver,
} from '@mantine/core';

// openKWL petrol, the brand and primary action colour (never red: red means a fault).
// [6] #0b6e78 carries white text at 5.97:1 and reads as link text on white at 5.97:1;
// [7] is the light-mode text tone (7.91:1 on white), [3] and [4] the dark-mode text tones
// (7.36:1 and 5.64:1 on the dark body #1c1c1c).
const brand: MantineColorsTuple = [
  '#e6f4f5', '#c9e7ea', '#97d0d6', '#63b7c0', '#3aa2ad',
  '#1a8a96', '#0b6e78', '#095a63', '#07474e', '#05343a',
];


// Neutral grey: #333333 text, #ededed fills.
// Mantine reads: [2] disabled bg, [4] input border, [5] placeholder, [6] dimmed text
const gray: MantineColorsTuple = [
  '#f5f5f5', '#ededed', '#dedede', '#c7c7c7', '#858585',
  '#666666', '#595959', '#424242', '#333333', '#1f1f1f',
];

// Dark scheme. Mantine reads: [0] text, [2] dimmed, [3] placeholder, [4] input border,
// [5] hover, [6] input / default-button bg, [7] body
const dark: MantineColorsTuple = [
  '#f2f2f2', '#d9d9d9', '#bdbdbd', '#9e9e9e', '#787878',
  '#363636', '#262626', '#1c1c1c', '#141414', '#0d0d0d',
];

// Mantine's own green/yellow/red/blue light variants fail 4.5:1 text contrast.
// Route them, and the primary colour, through the system's tokens instead.
const TOKEN_FOR: Record<string, string> = {
  green: 'success',
  yellow: 'warning',
  red: 'danger',
  blue: 'info',
};

const variantColorResolver: VariantColorsResolver = (input) => {
  const defaults = defaultVariantColorsResolver(input);
  const color = input.color || input.theme.primaryColor;
  const key = TOKEN_FOR[color];

  if (input.variant === 'light') {
    if (key) {
      return { ...defaults, background: `var(--app-${key}-soft)`, hover: `var(--app-${key}-soft)`, color: `var(--app-${key})`, border: 'transparent' };
    }
    if (color === 'brand') {
      return { ...defaults, background: 'var(--app-brand-soft)', hover: 'var(--app-brand-soft)', color: 'var(--app-brand-text)', border: 'transparent' };
    }
  }
  if (input.variant === 'filled' && color === 'red') {
    return { ...defaults, background: 'var(--app-danger-fill)', hover: 'var(--app-danger-fill-hover)', color: '#ffffff', border: 'transparent' };
  }
  return defaults;
};

export const theme = createTheme({
  primaryColor: 'brand',
  primaryShade: { light: 6, dark: 6 },
  colors: { brand, gray, dark },
  black: '#333333',
  white: '#ffffff',
  variantColorResolver,

  // Geist ships with the firmware (/fonts/, see mantine-overrides.css); system fonts are the fallback
  fontFamily: 'Geist, system-ui, -apple-system, "Segoe UI", Roboto, sans-serif',
  fontFamilyMonospace: '"Geist Mono", ui-monospace, Menlo, Consolas, monospace',
  fontSizes: { xs: rem(12), sm: rem(13), md: rem(14), lg: rem(16), xl: rem(18) },
  lineHeights: { xs: '1.33', sm: '1.4', md: '1.57', lg: '1.5', xl: '1.45' },
  headings: {
    fontFamily: 'Geist, system-ui, -apple-system, "Segoe UI", Roboto, sans-serif',
    fontWeight: '600',
    sizes: {
      h1: { fontSize: rem(24), lineHeight: '1.33' },
      h2: { fontSize: rem(18), lineHeight: '1.45' },
      h3: { fontSize: rem(15), lineHeight: '1.47' },
      h4: { fontSize: rem(13), lineHeight: '1.4' },
    },
  },

  spacing: { xs: rem(4), sm: rem(8), md: rem(16), lg: rem(24), xl: rem(32) },
  radius: { xs: rem(4), sm: rem(6), md: rem(8), lg: rem(12), xl: rem(999) },
  defaultRadius: 'md',
  shadows: {
    xs: '0 1px 2px rgba(0, 0, 0, 0.06)',
    sm: '0 1px 2px rgba(0, 0, 0, 0.06)',
    md: '0 4px 12px rgba(0, 0, 0, 0.08)',
    lg: '0 12px 32px rgba(0, 0, 0, 0.12)',
    xl: '0 12px 32px rgba(0, 0, 0, 0.12)',
  },
  focusRing: 'auto',
  cursorType: 'pointer',

  components: {
    Button: Button.extend({
      defaultProps: { size: 'sm' },
      styles: { root: { fontWeight: 600 } },
    }),
    Badge: Badge.extend({
      defaultProps: { variant: 'light', radius: 'sm', size: 'md' },
      styles: { root: { textTransform: 'none', fontWeight: 600, fontSize: 'var(--mantine-font-size-xs)' } },
    }),
    Card: Card.extend({ defaultProps: { withBorder: true, radius: 'lg', padding: 'lg' } }),
    Paper: Paper.extend({ defaultProps: { radius: 'lg' } }),
    Modal: Modal.extend({ defaultProps: { radius: 'lg', shadow: 'lg', centered: true } }),
    Input: Input.extend({ styles: { input: { fontSize: 'var(--mantine-font-size-md)' } } }),
    InputWrapper: Input.Wrapper.extend({
      styles: { label: { fontWeight: 500, marginBottom: rem(4) } },
    }),
    NumberInput: NumberInput.extend({
      styles: { input: { fontVariantNumeric: 'tabular-nums' } },
    }),
    // Neutral by default: view switches (profile, day grouping, settings section).
    // The Mode control on Overview passes color="brand"; everything else stays neutral.
    SegmentedControl: SegmentedControl.extend({
      defaultProps: { radius: 'md' },
      styles: { label: { fontWeight: 500 } },
    }),
    Table: Table.extend({
      defaultProps: { highlightOnHover: true, verticalSpacing: 'sm', horizontalSpacing: 'md' },
      styles: {
        th: { fontWeight: 500, color: 'var(--mantine-color-dimmed)', background: 'var(--app-surface-2)' },
        td: { fontVariantNumeric: 'tabular-nums' },
      },
    }),
    Alert: Alert.extend({ defaultProps: { variant: 'light', radius: 'md' } }),
    Title: Title.extend({ styles: { root: { letterSpacing: '-0.01em' } } }),
  },
});

// Tokens Mantine has no variable for, exposed as --app-*
export const resolver: CSSVariablesResolver = () => ({
  variables: {
    '--app-danger-fill': '#b00020',
    '--app-danger-fill-hover': '#8c0019',
  },
  light: {
    '--mantine-color-body': '#ffffff',
    '--mantine-color-text': '#333333',
    '--mantine-color-dimmed': '#595959',
    '--mantine-color-placeholder': '#666666',
    '--mantine-color-default-border': '#dedede',
    '--mantine-color-error': '#b00020',
    '--app-surface-0': '#f5f5f5',
    '--app-surface-2': '#ededed',
    '--app-brand': '#0b6e78',
    '--app-now-marker': '#0b6e78',
    '--app-brand-soft': '#e6f4f5',
    '--app-brand-text': '#095a63',
    '--app-focus-ring': '#0b6e78',
    '--app-success': '#276e2b',
    '--app-success-soft': '#e6f2e6',
    '--app-warning': '#8a5a00',
    '--app-warning-soft': '#fdf3dc',
    '--app-danger': '#b00020',
    '--app-danger-soft': '#fdecee',
    '--app-info': '#2563a8',
    '--app-info-soft': '#e7eff9',
    '--app-disabled-bg': '#e3e3e3',
    '--app-disabled-ink': '#7a7a7a',
    '--app-air-cold': '#1d5f99',
    '--app-air-warm': '#a3620a',
    '--app-air-frost': '#3a8fc4',
    '--app-level-0': '#f5f5f5',
    '--app-level-1': '#d4d4d4',
    '--app-level-2': '#a6a6a6',
    '--app-level-3': '#6b6b6b',
    '--app-level-4': '#333333',
    '--app-on-level-1': '#333333',
    '--app-on-level-2': '#1f1f1f',
    '--app-on-level-3': '#ffffff',
    '--app-on-level-4': '#ffffff',
  },
  dark: {
    '--mantine-color-body': '#1c1c1c',
    '--mantine-color-text': '#f2f2f2',
    '--mantine-color-dimmed': '#bdbdbd',
    '--mantine-color-placeholder': '#9e9e9e',
    '--mantine-color-default-border': '#363636',
    '--mantine-color-error': '#ff8a80',
    '--app-surface-0': '#141414',
    '--app-surface-2': '#262626',
    '--app-brand': '#3aa2ad',
    '--app-now-marker': '#3aa2ad',
    '--app-brand-soft': '#13292c',
    '--app-brand-text': '#63b7c0',
    '--app-focus-ring': '#3aa2ad',
    '--app-success': '#6cc282',
    '--app-success-soft': '#1a2e1f',
    '--app-warning': '#f2c062',
    '--app-warning-soft': '#352a12',
    '--app-danger': '#ff8a80',
    '--app-danger-soft': '#3d1517',
    '--app-info': '#7fb0ee',
    '--app-info-soft': '#172636',
    '--app-disabled-bg': '#383838',
    '--app-disabled-ink': '#8c8c8c',
    '--app-air-cold': '#6aa8e0',
    '--app-air-warm': '#e0a646',
    '--app-air-frost': '#b5dcf7',
    '--app-level-0': '#202020',
    '--app-level-1': '#3a3a3a',
    '--app-level-2': '#5e5e5e',
    '--app-level-3': '#9a9a9a',
    '--app-level-4': '#d9d9d9',
    '--app-on-level-1': '#f2f2f2',
    '--app-on-level-2': '#ffffff',
    '--app-on-level-3': '#141414',
    '--app-on-level-4': '#141414',
  },
});
