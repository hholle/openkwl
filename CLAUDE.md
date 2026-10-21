# CLAUDE.md

Project instructions for Claude Code are shared with other agents in AGENTS.md:

@AGENTS.md

## Design system: KWL
- Theme: web/src/theme/ (mantine-theme.ts, mantine-overrides.css, units.ts) is the reference. Use Mantine components with theme defaults; never hard-code colours, sizes, radii or shadows.
- Offline and small: Geist / Geist Mono from the unit (`/fonts/`), no CDNs or external requests; import components and icons by name; keep the gzipped UI small.
- Readings: always formatReading(); tabular-nums, unit after a no-break space, "—" for missing values.
- Colour: openKWL petrol #0b6e78 (palette `brand`, #3aa2ad on dark) is the brand and primary action colour; never use Helios' red or another red as an accent. Red is reserved for faults: the deeper danger red plus icon plus the word "Fault" — never colour alone.
- Copy: English, sentence case, plain words for homeowners, verb-first buttons; the same names for modes, levels and sensors as in Home Assistant.
- Anything that stops ventilation needs a confirm Modal stating how long.
- Product name "openKWL" (logo: lowercase "openkwl." with the petrol full stop). Logo: openkwl. wordmark and "k." symbol files only; never the Helios company logo, never retype the wordmark.
