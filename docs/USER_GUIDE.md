# User guide

How to use the openKWL web UI on your unit. To install it, see
[INSTALL.md](INSTALL.md).

## Open the web UI

- In a browser on your home network, go to `http://openkwl.local/`, or to the unit's IP
  address (your router's device list shows it). `openkwl` is the default host name; you can
  change it under *Settings › Network*.
- On a phone you can add it to the home screen (*Share › Add to Home Screen* on iOS,
  *Install app* on Android). It then opens like an app.
- The UI follows your device's light or dark mode.

**The first time**, the UI asks you to set a password (at least 10 characters). For safety,
this is possible only within 30 minutes after the unit was switched on. If you miss it, switch
the unit off and on again.

**Forgot the password?** Put an empty file named `RESETPW` in the root of the unit's SD card and
switch it on. Setup opens again; all other settings stay.

## Overview

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="images/overview-dark.png">
  <img alt="Overview page" src="images/overview-light.png" width="900">
</picture>

**Mode and level** (top left):

| Choice | What happens |
|---|---|
| **Auto** | The unit follows your weekly schedule |
| **Off** | Both fans stop |
| **1 – 4** | Runs at that level. Your schedule pauses until *Back to Auto* returns it (by default, at midnight). |
| **Party: more air** | A boost for a limited time (by default level 4 for 2 h 30 min), then back to what ran before |
| **Quiet: less noise** | Low or off for a limited time, then back |

In Auto, a small dot under a level shows the level the schedule runs right now.

**Air** (top right): the four air temperatures around the heat exchanger:

- **Outdoor air** comes in from outside, is warmed in the exchanger and goes to the rooms as
  **Supply air**.
- **Extract air** comes from the rooms, gives its heat to the exchanger and leaves as **Exhaust
  air**.
- The middle shows the **heat recovery** efficiency. Below it is the heat recovered right now.
  Top right is the estimated fresh-air flow ([how it is estimated](AIRFLOW.md)).

**Today:** the day's schedule. A line in the brand colour marks now.

**Status** (top right corner): *Running*, or a fault with its name, for example a stalled fan or
a disconnected sensor. Faults are shown with an icon and the word "Fault", never just a colour.

## Schedule

<img alt="Weekly schedule editor" src="images/schedule.png" width="900">

**Week:** what the unit does in Auto.
- **Layout:** the same schedule every day, weekdays and weekend, or each day separately.
- **Times:** drag a boundary in the bar, or type the times below it. Times snap to 15 minutes.
- **Days without times** continue the level that ran last, also over several days.
- **Profiles:** five schedules are kept, of which one is in use (*Using: …*). Standard 1–3 come
  from the stock firmware; *User defined 1–2* are yours. Switching profiles is quick, for
  example between a winter and a summer week.

**Holiday:** from a start date to an end date, the fans run at a set level for some minutes
every few hours, and are off in between. This keeps the air fresh while nobody is home.
Frost protection stays active. While a holiday runs, the mode and level can't be changed (on
the overview, in Home Assistant or by KNX); end the holiday here first.

**Party and Quiet:** the level and duration that the two buttons on the overview start with.

**Summer cooling:** in summer, the bypass lets cool outdoor air past the heat exchanger, so the
house isn't heated by the extract air. Set:
- the target room temperature;
- the lowest outdoor temperature allowed;
- the months in which the bypass may open.

> [!IMPORTANT]
> Turning summer cooling on asks you to confirm that the unit's rubber plug (condensate) is
> removed, as the stock firmware does. Check your unit's manual.

**Back to Auto:** when a level chosen by hand ends: at midnight, a set time after the last
change, or never. Only the clock running past midnight counts, not a correction of the clock
that lands on another day.

## History

*History* shows how the unit ran, in four views:

- **Last 24 hours** (the default): the four air temperatures, the recovered heat and the fan
  level in quarter hours, with the times the bypass was open or frost protection ran.
- **Live:** the same at a finer step, but only while the page is open in this browser.
- **Day:** one day of the past, with its figures (lowest, mean and highest temperatures,
  recovered heat, time per level).
- **Month:** each day's temperatures and recovered heat, hours per level, and for the month the
  recovered heat in kWh, the heat-recovery efficiency (an estimate from the temperatures), the
  heating degree-days (Gradtagzahl G20/15) and the hours of bypass and frost protection.

The unit keeps today in its memory and writes one record per day to the SD card after
midnight, so the card is barely written to. A restart loses today so far: that day is then
marked as partial. For history over years, use Home Assistant, which records every value.

## Settings

| Section | What it's for |
|---|---|
| **Filters** | Days left until the filter change (counted in fan running time) and the interval. After changing the filters, choose *Reset*; it is saved at once. |
| **Restart** | Restarts the controller. The fans stop for a few seconds. |
| **Home Assistant** | MQTT connection; see [HOME_ASSISTANT.md](HOME_ASSISTANT.md) |
| **Network** | Automatic (DHCP) or a fixed address; the host name for `http://<name>.local/` |
| **Clock** | Time from the network (NTP) or set by hand; time zone and summer time. The schedule needs a correct clock. |
| **Password** | Change it. This logs out every browser. |
| **Log out** | On this browser |
| **Fan levels** *(advanced)* | Fan voltage per level, and air-flow calibration ([AIRFLOW.md](AIRFLOW.md)). Usually set once by the installer. |
| **Frost protection** *(advanced)* | Thresholds of the stock frost protection. Keep the defaults unless you know why. |
| **Bypass actuator** *(advanced)* | Type and run time of the bypass motor |
| **Backup and restore** *(advanced)* | Download all settings as `CONFIG.INI`, or upload one. Passwords are not included. |
| **Firmware update** *(advanced)* | Install a new version from an `.okwl` file ([UPDATING.md](UPDATING.md)) |
| **Technical details** *(advanced)* | Firmware, chip, memory, SD card, network, MQTT, extension modules, fan speeds and voltages, the last reset cause, and the log. Useful for bug reports. |

Anything that stops the ventilation (restart, update, *Off*) asks first and says for how long.

## Good to know

- **Settings are saved on the SD card** in `HELIOS/CONFIG.INI`, a readable text file, about
  10 seconds after a change, with a backup copy that survives a power cut. The card is written
  only a few times a day, to make it last. To edit the file by hand, see
  [CONFIG_INI.md](CONFIG_INI.md).
- **After a power cut** the unit continues in the mode it had. The counters (operating hours,
  filter, energy) are saved when the power fails.
- **Without a network** the unit keeps running on its schedule; the network is only needed
  for the web UI, Home Assistant and setting the clock.
- **The original firmware is one SD card away** (it needs the front cover open); see
  [INSTALL.md, step 6](INSTALL.md#step-6-go-back-to-the-original-firmware).

More questions: [FAQ.md](FAQ.md).
