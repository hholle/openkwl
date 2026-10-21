# FAQ and troubleshooting

## General

**What is this, in one sentence?**
New software for the controller inside Helios KWL EC ventilation units (first easyControls
generation). It gives them a web UI, a REST API and Home Assistant integration, and
reimplements the stock firmware's observed control behaviour (same thresholds and states) in
new code. No Helios code or binaries are included.

**Do I need new firmware for Home Assistant at all?**
Not necessarily. The stock firmware has a Modbus TCP server, and existing projects use it:
[homeassistant-easycontrols](https://github.com/laszlojakab/homeassistant-easycontrols) (Home
Assistant, via HACS), [eazyctrl](https://pypi.org/project/eazyctrl/) (Python) and the
[openHAB binding](https://www.openhab.org/addons/bindings/modbus.helioseasycontrols/). They keep
the original firmware and need no opening of the unit.

**Can I use it with a fireplace, stove or gas appliance?**
No, not where a room-air-dependent fireplace, stove or gas appliance shares the air with the
ventilation. openKWL is not covered by the unit's building approval (DIBt general technical
approval, abZ). While it defrosts, the supply fan stops for up to several hours while the
exhaust fan keeps running; the under-pressure can pull flue gas into the rooms. Keep the
original firmware there.

**I have a KWL-BEC wall panel. Does it work?**
No. openKWL doesn't drive the panel, so it shows no current values and its keys do nothing.
Use the web UI (it works well on a phone) or Home Assistant. The panel works again with the
original firmware. Support for the panel is not planned.

**My unit has a pre-heater or post-heater.**
openKWL never switches a heater on. If your unit needs it for frost protection, stay with the
original firmware.

**Is it made or supported by Helios?**
No. It is an independent open-source project. "Helios" and "easyControls" are trademarks of
Helios Ventilatoren, used here only to say which units it runs on. Don't ask Helios for
support with it.

**Will it work on my unit?**
It has run on one KWL EC 300 W R for a few days (since 2026-10-17), not yet through a frost
season. The other first-generation easyControls models run the
same stock firmware image and should work, but they are untested. Units with heaters, the BEC
wall panel, easyControls 3.x and firmware 1.x are not supported. See
[Supported units](../README.md#supported-units). If you try another model, please
[report it](../../../issues/new?template=unit_report.yml).

**Does it need the internet or a cloud account?**
No. Everything runs on the unit. The only outside request is the time server (NTP), which you
can point at your router.

**Does my warranty survive?**
Maybe not. Replacing the firmware may end the manufacturer's warranty. Ask Helios or your
installer if the unit is still under warranty.

**Can I go back to the original firmware?**
Yes; that is designed to work whatever state openKWL is in. The stock bootloader is never
changed, and it installs the original image from an SD card just as it installs openKWL
([INSTALL.md, step 6](INSTALL.md#step-6-go-back-to-the-original-firmware)). Verified on one
unit (a dry run with the original image). The original firmware then starts with its old
settings, which are kept in a flash area that openKWL doesn't touch. With the normal build
running, going back from the web UI without opening the unit is also possible, but not yet
tried on a unit ([UPDATING.md](UPDATING.md#going-back)).

**Who may open the unit?**
Swapping the SD card means opening the front cover of a 230 V unit. That is work for a qualified
electrician (Elektrofachkraft), or whoever is allowed to do it under your local law. See
[INSTALL.md, Safety](INSTALL.md#safety).

**Was it written with AI?**
Yes, with substantial help from an AI coding assistant (Claude Code), directed and reviewed by
the maintainer and tested on one unit. Reviews of the control logic and the update path are
especially welcome.

**Why plain HTTP and not HTTPS?**
TLS needs about 35 KB of RAM and more flash than is left in the 223.5 KB slot. It also needs a
certificate that browsers trust for a device in your home. Put a reverse proxy with TLS in
front of the unit (Caddy, nginx, Home Assistant), or use a VPN for access from outside. See
[SECURITY.md](../SECURITY.md).

**Why Rust?**
Memory safety without a heap; the same code runs in the simulator and in host tests; and the
`no_std` ecosystem (Embassy, smoltcp) is mature enough. The catch is size, which needs care
([DEVELOPMENT.md](DEVELOPMENT.md#the-flash-budget)).

## Using it

**How do I reach the web UI?**
`http://openkwl.local/` (the default host name), or the IP address from your router's device
list. `.local` names may not work on some Android devices; use the IP address there.

**I forgot the password.**
Put an empty file named `RESETPW` in the root of the SD card and switch the unit on. Setup opens
again; all other settings stay.

**The setup page says setup is closed.**
For safety, the first password can only be set within 30 minutes after power-on. Switch the
unit off and on, then set it.

**Why does the unit go back to Auto at midnight?**
That's *Schedule › Back to Auto*, as in the stock firmware. You can change it to a set time
after the last change, or turn it off.

**How accurate is the air-flow figure?**
It's an estimate from the fan speed: about ±15–25 %, or about ±10 % after one measurement.
See [AIRFLOW.md](AIRFLOW.md).

**Where are my settings stored?**
On the SD card, in `HELIOS/CONFIG.INI`: a readable text file you can also edit on a computer.
*Settings › Backup and restore* downloads it.

## Troubleshooting

| Symptom | What to do |
|---|---|
| No web UI after installing | Wait 10 minutes; the bootloader takes a few. Then see [INSTALL.md, If something goes wrong](INSTALL.md#if-something-goes-wrong). |
| The status shows a fault | Its name says what's wrong. *Settings › Technical details* has the details (fan speeds, sensors, SD card). Fan stalls and sensor faults are real hardware problems, as in the stock firmware. |
| "Clock not set", the schedule doesn't run | The unit couldn't reach a time server. Check *Settings › Clock* and the network, or set the time by hand. Until then, Auto runs a fixed fallback level. |
| `CONFIG.INI` edited by hand, now a notice on the overview | The file has an error; the last good copy (`CONFIG.BAK`) is in use. Fix the file, or upload a good one under *Settings › Backup and restore*. |
| Settings don't stick after a restart | Check *Technical details › SD card*. Settings are saved about 10 seconds after a change; a power cut within those seconds can lose it. |
| Home Assistant doesn't see the unit | [HOME_ASSISTANT.md, Troubleshooting](HOME_ASSISTANT.md#troubleshooting) |
| An update is refused | The message says why; nothing has changed. See [UPDATING.md](UPDATING.md#what-can-go-wrong-and-what-happens-then). |
| The unit restarts by itself | *Technical details* shows the last reset cause. A watchdog restart means something hung: please [open a bug report](../../../issues/new?template=bug_report.yml) with the *Technical details* and the log. |
| Supply and exhaust seem swapped | Not yet confirmed on every unit. Please report it with your model. Go back to the stock firmware meanwhile. |

**Collecting information for a bug report:** *Settings › Technical details* (copy the whole
page) and *Show log*. For start-up problems, the `sd-log` debug build writes a detailed log to
the card ([firmware/README.md](../firmware/README.md#debugging-on-the-unit)).
