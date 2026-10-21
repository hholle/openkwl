// Browser tab titles: "Page · openKWL" (KWL design system, Favicons README).

export const PRODUCT = 'openKWL';

export function documentTitle(page: string | null): string {
  return page ? `${page} · ${PRODUCT}` : PRODUCT;
}
