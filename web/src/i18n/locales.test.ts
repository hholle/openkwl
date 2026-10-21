import { describe, expect, it } from 'vitest';
import { resources } from '.';

// Every key path of a dictionary, plural suffixes (`_one`, `_other`) folded into their base key.
function keys(o: object, prefix = ''): string[] {
  return Object.entries(o).flatMap(([k, v]) =>
    typeof v === 'object' && v !== null ? keys(v, `${prefix}${k}.`) : [`${prefix}${k.replace(/_(zero|one|two|few|many|other)$/, '')}`],
  );
}

// The placeholders a text uses, e.g. {{n}}.
const placeholders = (s: unknown) => [...String(s).matchAll(/\{\{(\w+)\}\}/g)].map((x) => x[1]).sort();

function leaves(o: object, prefix = ''): [string, unknown][] {
  return Object.entries(o).flatMap(([k, v]) => (typeof v === 'object' && v !== null ? leaves(v, `${prefix}${k}.`) : [[`${prefix}${k}`, v] as [string, unknown]]));
}

describe('translations', () => {
  for (const ns of Object.keys(resources.en) as (keyof typeof resources.en)[]) {
    it(`German has every English text in "${ns}", with the same placeholders`, () => {
      const en = resources.en[ns];
      const de = resources.de[ns];
      expect([...new Set(keys(de))].sort()).toEqual([...new Set(keys(en))].sort());
      const deLeaves = new Map(leaves(de));
      for (const [key, text] of leaves(en)) {
        const other = deLeaves.get(key) ?? deLeaves.get(key.replace(/_one$/, '_other'));
        if (other !== undefined) expect(placeholders(other), key).toEqual(placeholders(text));
      }
    });
  }
});
