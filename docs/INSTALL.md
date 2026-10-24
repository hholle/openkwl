# Trying openKWL on your unit with a second SD card

openKWL is **experimental**: it first booted on one unit (KWL EC 300 W R) on 2026-10-17 and
has run there for a few days; every other model is untested. Expect something not to work, and
keep the first session short (an hour, while you are at home).

**You install it at your own risk.** It comes without any warranty, it is not made or supported
by Helios, and replacing the firmware may end the manufacturer's warranty. Read *Safety* below
before you open the unit.

The plan: your original SD card stays untouched in a drawer. You test with a second card, and
you go back by flashing the original firmware from that second card.

**How often the cover is opened.** Each card swap means opening the front cover, with the unit
off mains:

| | Openings |
|---|---|
| First installation with the dry run (recommended, step 3) | 2: second card with the original image in; then openKWL onto it |
| First installation without the dry run | 1 |
| Updates and going back to stock from the web UI (normal build, [UPDATING.md](UPDATING.md)) | 0; not yet tried on a unit |
| Going back by SD card (step 6), reading the `sd-log` debug log, or moving from the debug build to the normal build | 1 each |

To open the cover as rarely as possible, read the settings before you open it (*Before you open
the unit*), and install the normal build rather than the `sd-log` debug build (step 4).

## Why this is safe, and what isn't

The controller has a **bootloader** in flash (`BASBL110`, version 1.10) that is never written by
openKWL or by this procedure. What it does, as read from its code in the stock image:

- It runs **before** any firmware at every power-on. It installs an image only when the first
  line of `settings.ini` on the card says `FwBasis=<file>` and that file exists on the card.
- After programming, and only then, it renames the file to `basback.bin`, so the next power-on
  starts the installed firmware without flashing again.

What follows from that:

| If … | then … |
|---|---|
| openKWL hangs, crashes, has no network or the fans misbehave | Switch off and flash the original firmware from a card (step 6). The bootloader runs before openKWL, so a broken openKWL cannot block this. |
| power is cut while the bootloader is flashing | The file still has its name, so the next power-on should flash again (read from the bootloader's code; not tried). |
| the bootloader refuses an image (wrong checksum) | The unit blinks in the bootloader until a card with a good image goes in. |
| you go back to the original firmware | It starts with its old settings and password: they are in a flash area (sector 1) and on the original card, and openKWL touches neither. |

Not covered:
- **A KWL-BEC wall panel doesn't work with openKWL.** It stays connected but is not driven: it
  shows no current values and its keys do nothing. Control the unit from the web UI or Home
  Assistant instead. The panel works again as soon as the original firmware is back.
- **Pre- and post-heaters (EVH, KWL-EM/ERW, EH-Basis, SEWT/LEWT) are never switched on.** If your
  unit has one fitted and needs it for frost protection, stay with the original firmware. A
  heater that is only *configured* in the stock settings but not fitted doesn't matter.
- **The KNX module (KWL-KNX) is untested with openKWL.** It is driven like the stock firmware
  (*Settings › Technical details › Extension modules* shows whether it answers). If it does not
  work, it is not damaged and works again with the original firmware.
- **Untested hardware details**, for example which fan output is supply and which is exhaust,
  and the bypass motor direction. Watch the unit during the first session.
- The behaviour above was read from the bootloader's code. Installing the original image from a
  second card was tried on one unit (the dry run of step 3); a power cut while flashing was not.
  **Step 3 tries it on your unit with the original firmware first**, before anything new goes on
  it.

## Safety

- **Mains voltage.** The SD card sits on the controller behind the front cover of a 230 V unit.
  Opening the cover is work for a **qualified electrician (Elektrofachkraft)**, or whoever is
  allowed to do it under your local law; Helios' manual restricts it to qualified electricians.
  Disconnect the unit from mains on all poles before opening the cover (fuse off and secured, or
  plug out), and follow the unit's manual.
- **ESD.** Touch earthed metal before touching the board or the card.
- **Never switch off or pull the card while the bootloader is flashing:** from power-on until
  the web UI answers (a few minutes). The LEDs blink while it flashes.
- **Fireplaces, stoves and gas appliances.** openKWL is not covered by the unit's building
  approval (DIBt general technical approval, abZ). Don't use it where a room-air-dependent fireplace,
  stove or gas appliance shares the air with the ventilation: while openKWL defrosts the heat
  exchanger, the supply fan stops for up to several hours (180 minutes per cycle with the stock
  parameters) while the exhaust fan keeps running. The resulting under-pressure can pull flue gas
  into the rooms. Stay with the original firmware there.
- **Deep frost.** Don't leave the unit unattended in deep frost with openKWL: its frost
  protection has not been through a frost season on a unit yet.
- **After installing, check the unit:** both fans turn, supply and exhaust are the right way
  round, and the four temperatures make sense. If not, go back (step 6).

## What you need

- **A second SD card.** Same kind as the original (full-size SD or adapter), FAT32. Prefer 4 GB
  or smaller: Helios notes that older bootloaders don't handle larger cards.
- A card reader, and a computer (macOS or Linux) with this repository and
  [mise](https://mise.jdx.dev). Everything else (Rust for the controller, Node, pnpm, the web
  UI's packages) is installed by `firmware/card.sh` on its first run.
- **The original firmware image** for going back: version 2.27, the current one for the whole
  first easyControls generation. Two ways to get it:
  - **From Helios:** the firmware download for easyControls on Helios' website
    (easycontrols.net, the *Offline-Updater* / firmware download for the first generation,
    easyControls 2.x). Use the main-controller image (`BAS…bin`) from that download.
  - **From your own card:** `basback.bin` on the original card (after step 1, in
    `backup/original-card/`) is the image the bootloader last installed. Use it only if your unit
    shows version 2.27 (stock web UI, *Technische Info*); the checksum below tells you.

  Save it as `backup/BAS00227.bin`, check it, and keep a copy somewhere safe:

  ```sh
  md5 backup/BAS00227.bin        # Linux: md5sum. Must be 5dbf83d504f07f923a3c5e5d3202c927
  mise exec -- cargo run -p openkwl-pack -- verify backup/BAS00227.bin   # must say OK
  ```

  The file is 228,868 bytes. If your unit still runs an older 2.x version, going back installs
  2.27, as Helios' own update would. Helios' terms apply to the image; this project does not
  distribute it.
- For the debug log: `cargo install defmt-print` once.

## Before you open the unit

While the original firmware still runs with the original card, read its settings (fan voltages,
schedule, bypass, network). This only reads, without logging in, and needs no opening:

```sh
mise exec -- cargo run -p openkwl-import -- --from http://<unit address>/ \
    --out backup/import --save-raw backup/stock-values
```

`backup/stock-values` keeps the raw values, so step 4 can redo the import offline with the
parameter file from the card. Read the printed notes. Without this, openKWL starts with default
settings.

## Step 1: Back up the original card

1. Switch off and open the cover (see *Safety*), take the original card out. The unit stays off
   until the second card goes in.
2. On the computer, copy **all** files from it to `backup/original-card/`. A full image is even
   better (macOS):

   ```sh
   diskutil list                      # find the card, e.g. /dev/disk4 — check the size!
   diskutil unmountDisk /dev/disk4
   sudo dd if=/dev/rdisk4 of=backup/original-card.img bs=1m
   ```
3. Eject it, label it "original", and keep it out of the unit for now.

## Step 2: Prepare the second card

1. Format it FAT32 (macOS Disk Utility: *MS-DOS (FAT)*, scheme *Master Boot Record*).
2. Copy **all** files from `backup/original-card/` onto it. The unit then finds everything it
   expects (parameter files, `Web*/`, module images), whichever firmware runs.

The second card now behaves exactly like the original.

## Step 3: Dry run with the original firmware

This proves, on your unit and with the second card, that the bootloader installs from a card,
before anything new is involved. It installs the same original version again. Recommended; it
costs one more opening of the cover (the card comes out again for step 5) and about
10 minutes without ventilation. Without it, skip to step 4 and put openKWL on the second card
during the same opening.

1. Put the original image on the second card under a new name and check it:

   ```sh
   cp backup/BAS00227.bin /Volumes/<card>/BAS00227.bin
   mise exec -- cargo run -p openkwl-pack -- verify /Volumes/<card>/BAS00227.bin
   md5 /Volumes/<card>/BAS00227.bin   # Linux: md5sum. Must be 5dbf83d504f07f923a3c5e5d3202c927
   ```
2. In the card's `settings.ini`, change only the first line to `FwBasis=BAS00227.bin`. Leave the
   other lines (`FwBecU1=`, `FwErwMd=`, `FwExSen=`, `erase=`) as they are.
3. Second card into the unit, cover closed, power on. Wait 5 minutes.
4. Check: the original web UI answers with version 2.27 and your settings. On the card,
   `BAS00227.bin` is gone and `basback.bin` has its size (228,868 bytes).

If this worked, going back is proven. If the unit does not come back, see *If something goes
wrong*; don't continue.

## Step 4: Build the openKWL files

On the computer, in this repository, with the second card's `settings.ini` (any copy from the
original works, e.g. `backup/original-card/settings.ini`). The script builds the firmware for
the controller, turns it into an image the bootloader accepts (padded to the stock size, with
its checksum, checked with `openkwl-pack verify`), builds the web UI and packs it into 8.3 file
names:

```sh
# Normal build: later updates and going back from the web UI, no card log
firmware/card.sh backup/original-card/settings.ini
# Debug build: writes its log to the card, but has no web update
firmware/card.sh backup/original-card/settings.ini --sd-log
```

Which one? The **normal build** keeps the cover closed for later updates (once that path is
proven) and still has *Technical details* and *Show log* in the web UI. The **`sd-log` build**
also logs the start-up to the card (`BOOT.LOG`, `DEFMT.LOG`), which helps when openKWL doesn't
come up, but reading that log and moving to the normal build later each need the cover open.

It leaves in `card/`: `BAS90001.bin`, `settings.ini` (only `FwBasis=` changed), `WWW/` and
`openkwl-fw.elf`. **Keep `openkwl-fw.elf`** next to the log you collect later: the log can only
be decoded with the ELF of exactly that build.

Then redo the settings import from *Before you open the unit*, now with the parameter file
from the card backup (offline, from the saved values):

```sh
mise exec -- cargo run -p openkwl-import -- --from-dir backup/stock-values \
    --out backup/import --cwp backup/original-card/<article>.CWP
```

`<article>.CWP` is the parameter file on your card (e.g. `04232001.CWP`). Read the printed notes.

## What is on the card

FAT32 (MBR), as the units ship it. openKWL only reads the stock files and writes nothing
outside `HELIOS/`.

| Path | From | Purpose |
|---|---|---|
| `settings.ini` | stock | Line 1 `FwBasis=<file>` tells the bootloader what to install; the other lines name module images. |
| `BAS90001.bin` | `card.sh` | The openKWL image. After installing, the bootloader renames it to `basback.bin`. |
| `basback.bin` | bootloader | The last image it installed. |
| `*.HGP`, `*.HWP`, `*.CGP`, `*.CWP`, `Web*/`, module images, logs | stock | Used by the original firmware; openKWL leaves them as they are (`*.CWP` feeds the import). |
| `WWW/` | `card.sh` | Web UI, gzipped, 8.3 names: `INDEX.HTM`, `APP.JS`, `APP.CSS`, icons (`FAVICON.ICO`, `FAVICON.SVG`, `APPLETCH.PNG`, `ICON192.PNG`, `ICON512.PNG`, `ICONM512.PNG`), `MANIFEST.WEB`, fonts `GEISTVAR.WF2`, `GEISTMNO.WF2`, `OFL.TXT`. Read-only. |
| `HELIOS/CONFIG.INI` | import, then openKWL | All settings, plain text; edits are read at boot. |
| `HELIOS/CONFIG.BAK` | openKWL | Copy of `CONFIG.INI`, written first on every save; used if `CONFIG.INI` is empty or broken. |
| `HELIOS/SECRETS.INI`, `SECRETS.BAK` | openKWL | MQTT password (never shown in the UI), and its copy. |
| `HELIOS/AUTH.BIN`, `AUTH.BAK` | openKWL | The web password (salted hash), and its copy. Put `RESETPW` in the root to set a new one. |
| `HELIOS/STATEA.BIN`, `STATEB.BIN` | import, then openKWL | Counters and mode, written alternately so a power cut never loses both. |
| `HELIOS/DAYS.BIN` | openKWL | History: one record per day (96 quarter-hour points and the day's totals), written once after midnight; up to 0.75 MB. |
| `HELIOS/BOOT.LOG` | openKWL | One line per start-up step, plain text: how far each start got (`firmware/README.md`, *Debugging on the unit*). |
| `HELIOS/DEFMT.LOG` | `sd-log` build | Debug log (binary; decode with the matching ELF). |
| `RESETPW` | you | Empty file: the next start clears the web password and deletes this file. |

## Step 5: Install and test

1. With the dry run: switch off, open the cover, second card into the computer. Without it:
   the second card is still in the computer from step 2. Copy onto it:

   | From | To the card |
   |---|---|
   | `card/BAS90001.bin` | root |
   | `card/settings.ini` | root (replaces it) |
   | `card/WWW/` | `WWW/` |
   | `backup/import/HELIOS/` | `HELIOS/` |

   Don't copy `openkwl-fw.elf`. Leave everything else on the card as it is.
2. Card into the unit, cover closed, power on. Wait 5 minutes; don't switch off meanwhile.
3. Open `http://<unit address>/` (the same address as before; the factory MAC address is kept)
   or `http://<hostname>.local/` (`hostname` in `HELIOS/CONFIG.INI`).
4. Set a new password. Setup is open for 30 minutes after power-on; if you miss it, switch off
   and on again.
5. Check, and note what you see:
   - the four temperatures are plausible (compare with a thermometer or the values you knew);
   - choose level 1, then 4: both fans follow; *Settings › Technical details* shows both rpm;
   - outdoor air goes in, stale air out (hold a tissue at a valve; supply vs. exhaust may be
     swapped);
   - *Technical details › Board*: SD card, mains pulses, PHY address.
6. Leave it running for the session, then go back (step 6) or keep it if all is well.

**Collecting the debug log** (`sd-log` build; needs the cover open): switch off, card into the
computer, then

```sh
defmt-print -e card/openkwl-fw.elf < /Volumes/<card>/HELIOS/DEFMT.LOG > session.txt
```

Panics and hard faults are in there too (`firmware/README.md`, *Debugging on the unit*). The
web UI's *Settings › Technical details › Show log* has the app's own events.

## Later updates: from the web UI

Once the normal build runs (not the `--sd-log` debug build, which has no room for it), new
versions are designed to install without a card swap. This is tested in the simulator, not yet
on a unit:

1. Build the update file: `firmware/card.sh <settings.ini>` also writes `card/openkwl.okwl`
   (firmware and web UI in one file).
2. *Settings › Firmware update › Choose update file*, then confirm *Install update*. The unit
   stores and checks the whole file while it uploads; ventilation keeps running. A damaged or
   wrong file is refused and nothing changes.
3. If the file is fine, the unit restarts by itself: the bootloader installs the image (the fans
   stop for about a minute; don't switch the unit off meanwhile), and the page reloads with the
   new web UI.

What it changes on the card: the image (`BAS90001.BIN`/`BAS90002.BIN`), the web UI folder
(`WWW/` or `WWW2/`), and the first line of `settings.ini`, after copying the original once to
`HELIOS/SETTINGS.BAK`. If anything goes wrong, step 6 below is still there. Details:
[UPDATING.md](UPDATING.md).

## Step 6: Go back to the original firmware

**Without opening the unit** (normal build running, web UI reachable): install a `.okwl` that
holds only the original image ([UPDATING.md, Going back](UPDATING.md#going-back)). Designed for
this, but not yet tried on a unit. The original firmware then runs from the second card, which
holds everything the original card has; the original card can stay in the drawer.

**By SD card**, which works whatever state openKWL is in, because the bootloader runs before any
firmware (tried on one unit with the dry run):

1. Switch off, open the cover, second card into the computer.
2. Copy the original image (`backup/BAS00227.bin`) to it, verify it, and set the first line of
   `settings.ini` to `FwBasis=BAS00227.bin`, as in step 3.
3. Card into the unit, power on, wait 5 minutes: the original firmware starts.
4. Optional, one more opening: switch off and put the **original card** back. Its
   `settings.ini` names a file that is no longer on it, so nothing is flashed: the unit runs as
   before the test. The second card works too; it holds the same stock files.

To try a new openKWL build later, repeat steps 4 and 5 with the second card (or, while the
normal build runs, install its `.okwl` from the web UI). `HELIOS/` on it
keeps openKWL's settings and password between tests.

## If something goes wrong

| Symptom | What to do |
|---|---|
| No web UI after 10 minutes | Note the LEDs (`firmware/README.md`, *Debugging on the unit*), switch off, look at the card. Image file gone and `basback.bin` the same size: it was installed; `HELIOS/BOOT.LOG` shows how far openKWL got, and the address is in the router's DHCP list. Image still there: it was not installed; check that `FwBasis=` is the first line and the name matches exactly. |
| LEDs keep blinking, nothing answers | The bootloader refused the image. Step 6 with the original image. |
| openKWL runs but something is wrong | Collect the debug log and the *Technical details*, then step 6. |
| Forgot the openKWL password | Put an empty file `RESETPW` in the card root and power on: setup opens again, other settings stay. |
| `CONFIG.INI` edited by hand and broken | openKWL falls back to `HELIOS/CONFIG.BAK` and says so on the overview. |
| The original firmware does not come back | Put the original card back unchanged and power on. If that fails too, write `backup/original-card.img` back onto the original card with `dd` and try again. |
