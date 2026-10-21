import { Anchor, Box, Card, Divider, Group, NavLink, Stack, Text, Title } from '@mantine/core';
import { useMediaQuery } from '@mantine/hooks';
import { IconChevronRight } from '@tabler/icons-react';
import { Fragment, type ReactNode, useEffect } from 'react';
import type { BadgeColor } from '../lib/faults';
import { link, type Page, pathOf } from '../lib/router';
import { documentTitle } from '../lib/title';
import { DotBadge } from './StatusBadge';
import { TAP } from '../lib/tap';

export interface SectionRow {
  id: string;
  title: string;
  /** The current value ("50 days left"), not a description. */
  summary?: ReactNode;
  badge?: { label: string; color: BadgeColor };
  /** Rows without a page (Log out) run this instead of opening a section. */
  action?: { onClick: () => void; icon: ReactNode };
}

export interface SectionGroup {
  label?: string;
  note?: string;
  rows: SectionRow[];
}

/**
 * Schedule and Settings: a list of topics with live summaries. From `md` the list sits beside
 * the open section (the first row unless the URL names one); on phones the list opens each
 * section as its own page with a back link.
 */
export function SectionList({
  page,
  title,
  intro,
  groups,
  selected,
  render,
}: {
  page: Exclude<Page, 'overview'>;
  title: string;
  intro: ReactNode;
  groups: SectionGroup[];
  selected: string | null;
  render: (id: string) => ReactNode;
}) {
  const wide = useMediaQuery('(min-width: 62em)', true, { getInitialValueInEffect: false });
  const rows = groups.flatMap((g) => g.rows).filter((r) => !r.action);
  const open = rows.find((r) => r.id === selected) ?? (wide ? rows[0] : undefined);

  useEffect(() => {
    document.title = documentTitle(open?.title ?? title);
  }, [open?.title, title]);

  const list = (
    <Card padding={0} w={wide ? 300 : undefined} style={{ flexShrink: 0 }}>
      {groups.map((g, gi) => (
        <Fragment key={g.label ?? gi}>
          {g.label && (
            <Box px="md" pt="md" pb={g.note ? 0 : 6}>
              <Text size="xs" fw={600} c="var(--mantine-color-placeholder)">
                {g.label}
              </Text>
              {g.note && (
                <Text size="xs" c="var(--mantine-color-placeholder)" pb={6}>
                  {g.note}
                </Text>
              )}
            </Box>
          )}
          {g.rows.map((r, ri) => (
            <Fragment key={r.id}>
              {(ri > 0 || (!g.label && gi > 0)) && <Divider />}
              <Row row={r} page={page} active={!r.action && r.id === open?.id} />
            </Fragment>
          ))}
        </Fragment>
      ))}
    </Card>
  );

  if (!wide && open) {
    return (
      <Stack gap="lg">
        <div>
          <Anchor {...link(pathOf(page))} size="sm" fw={500} c="var(--app-brand-text)" style={TAP}>
            ‹ {title}
          </Anchor>
          <Title order={1} mt={4}>
            {open.title}
          </Title>
        </div>
        {render(open.id)}
      </Stack>
    );
  }

  return (
    <Stack gap="lg">
      <div>
        <Title order={1}>{title}</Title>
        <Text c="dimmed">{intro}</Text>
      </div>
      {wide && open ? (
        <Group align="flex-start" gap="lg" wrap="nowrap">
          {list}
          <Box style={{ flex: 1, minWidth: 0 }} maw={640}>
            {render(open.id)}
          </Box>
        </Group>
      ) : (
        list
      )}
    </Stack>
  );
}

function Row({ row, page, active }: { row: SectionRow; page: Exclude<Page, 'overview'>; active: boolean }) {
  const right = (
    <Group gap="xs" wrap="nowrap">
      {row.badge && <DotBadge color={row.badge.color}>{row.badge.label}</DotBadge>}
      {!row.action && <IconChevronRight size={16} stroke={1.6} color="var(--mantine-color-placeholder)" />}
    </Group>
  );
  const label = (
    <Text size="md" fw={active ? 600 : 500}>
      {row.title}
    </Text>
  );
  const description = row.summary ? (
    <Text size="xs" c="var(--mantine-color-placeholder)" component="span">
      {row.summary}
    </Text>
  ) : undefined;
  if (row.action) {
    return (
      <NavLink component="button" onClick={row.action.onClick} label={label} leftSection={row.action.icon} mih={48} px="md" />
    );
  }
  return (
    <NavLink
      component="a"
      {...link(pathOf(page, row.id))}
      label={label}
      description={description}
      rightSection={right}
      active={active}
      aria-current={active ? 'page' : undefined}
      mih={48}
      px="md"
    />
  );
}
