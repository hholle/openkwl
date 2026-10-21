import { Box } from '@mantine/core';
import wordmark from '../assets/openkwl-wordmark.svg';
import reversed from '../assets/openkwl-wordmark-reversed.svg';

/**
 * The openkwl. wordmark (never the Helios company logo): the charcoal version on light
 * grounds, the reversed one in dark mode. The SVGs are inlined into the bundle. A plain <img>
 * keeps its aspect ratio wherever it sits (an <Image> would stretch to the column width).
 */
export function Wordmark({ height }: { height: number }) {
  // No `display` here: darkHidden/lightHidden hide the other version with display: none.
  const style = { height, width: 'auto' } as const;
  return (
    <>
      <Box component="img" src={wordmark} alt="openKWL" style={style} darkHidden />
      <Box component="img" src={reversed} alt="openKWL" style={style} lightHidden />
    </>
  );
}
