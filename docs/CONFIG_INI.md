# `CONFIG.INI` reference

All settings of openKWL, in `HELIOS/CONFIG.INI` on the unit's SD card. You normally change them
in the web UI. To edit the file by hand, take the card out (unit switched off), change it on a
computer, and put it back: the unit reads it at start-up.

- **Format:** `key = value`, one per line, in `[sections]`. Lines starting with `#` or `;` are
  comments, and so is everything after ` #` on a line. Your own comments are not kept: the unit
  rewrites the file when a setting changes in the web UI.
- **Missing keys** keep their default; unknown keys and sections are ignored (and noted in the
  log).
- **A file that doesn't load** (a typo, a value out of range): the unit uses its backup
  `CONFIG.BAK` and shows a notice on the overview. Your file stays as it is, so you can fix it.
- **Backup and power cuts:** every save writes `CONFIG.BAK` first, then `CONFIG.INI`. If
  `CONFIG.INI` is empty after a power cut, the unit restores it from the backup.
- **Passwords** are not in this file: the MQTT password is in `SECRETS.INI`, the web password
  (as a hash) in `AUTH.BIN`.
- **Download and upload** the file in the web UI under *Settings › Backup and restore*.

Ranges below are the ones the web UI and the API accept ([`api/openapi.yaml`](../api/openapi.yaml)).

## `[network]`

Takes effect after a restart.

| Key | Values | Meaning |
|---|---|---|
| `dhcp` | `true` / `false` | Address from the router (`true`), or the fixed address below |
| `hostname` | letters, digits, `-` | Name on the network: `http://<hostname>.local/`; also the Home Assistant device name |
| `address`, `netmask`, `gateway`, `dns` | IPv4, e.g. `192.168.1.50` | Only used with `dhcp = false` |

## `[time]`

| Key | Values | Meaning |
|---|---|---|
| `ntp` | `true` / `false` | Set the clock from a time server |
| `ntp_server` | host name or IPv4 | Empty: the router (if it answers), else `pool.ntp.org` |
| `utc_offset_min` | −720 … 840 | Standard time offset in minutes, e.g. `60` for Central Europe |
| `eu_dst` | `true` / `false` | EU summer time (last Sunday of March to last Sunday of October) |

## `[mqtt]`

Home Assistant; see [HOME_ASSISTANT.md](HOME_ASSISTANT.md). The password is in `SECRETS.INI`.

| Key | Values | Meaning |
|---|---|---|
| `enabled` | `true` / `false` | Connect to the broker |
| `host`, `port` | name or IPv4, 1 … 65535 | The broker |
| `username` | text | MQTT user |
| `client_id` | letters, digits, `-`, `_` | Unique on the broker; also the node ID in discovery topics |
| `base_topic` | e.g. `openkwl` (the default) | State and command topics live below it |
| `discovery`, `discovery_prefix` | `true` / `false`, e.g. `homeassistant` | Home Assistant auto-discovery |

Text values can't contain line breaks, spaces at either end, or ` #`.

## `[fans]`

| Key | Values | Meaning |
|---|---|---|
| `supply_volts`, `extract_volts` | four values in V, 0 … 10 | Fan voltage for levels 1–4; below 1.65 V a fan is off |
| `airflow_m3h` | four values in m³/h | Measured supply air flow per level; `0` = estimate from the fan speed |
| `m3h_per_krpm` | number | Air flow per 1000 rpm for the estimate ([AIRFLOW.md](AIRFLOW.md)); `0` = no estimate |

## `[programs]`

Defaults for the Party and Quiet buttons.

| Key | Values | Meaning |
|---|---|---|
| `party_level`, `quiet_level` | 0 … 4 | Level while the program runs |
| `party_min`, `quiet_min` | 5 … 180 | Duration in minutes |

## `[bypass]`

Summer cooling. **Before using it, remove the unit's rubber plug (condensate), as its manual
describes.**

| Key | Values | Meaning |
|---|---|---|
| `enabled` | `true` / `false` | Cooling with outdoor air allowed |
| `target_c` | 10 … 35 | Room (extract air) temperature above which the bypass opens |
| `min_outdoor_c` | 0 … 30 | Outdoor air must be at least this warm |
| `close_if_outdoor_warmer` | `true` / `false` | Close when outdoor air is more than 2 K warmer than the room |
| `actuator` | `motor`, `motor_reversed`, `analog` | Bypass actuator type |
| `run_time_s` | 5 … 600 | Actuator run time |
| `season` | `MM-DD..MM-DD`, or empty | Months in which the bypass may open; empty = all year |
| `supply_offset_k` | number | Stock setting, kept but not used yet |

## `[frost]`

Frost protection, with the stock firmware's thresholds and states. Keep the defaults unless you know why.

| Key | Default | Meaning |
|---|---|---|
| `arm_below_c` | −3.1 | Outdoor temperature below which frost protection watches |
| `entry_pct`, `hold_pct`, `exit_pct` | 58, 60, 60 | Heat-recovery efficiency thresholds (icing suspected below `entry_pct`) |
| `boost_delay_s` | 900 | Exhaust boost before defrosting |
| `defrost_s` | 10800 | Defrost duration (supply fan stopped) |
| `test_run_s` | 900 | Test run after defrosting |
| `exhaust_boost_v` | 1.2 | Extra voltage on the exhaust fan while boosting |

## `[holiday]`

| Key | Values | Meaning |
|---|---|---|
| `enabled` | `true` / `false` | Holiday program on |
| `start`, `end` | `YYYY-MM-DD` | First and last day (inclusive) |
| `level` | 0 … 4 | Level during the bursts |
| `interval_h`, `active_min` | 1 … 24, 5 … 300 | A burst of `active_min` minutes every `interval_h` hours; off in between |

## `[filter]`

| Key | Values | Meaning |
|---|---|---|
| `interval_months` | 0 … 24 | Filter change after this many 30-day months of fan running time; `0` = no reminder |

## `[schedule]`

| Key | Values | Meaning |
|---|---|---|
| `active_profile` | 1 … 5 | The weekly profile in use (1–3 Standard, 4–5 user defined) |
| `auto_at_midnight` | `true` / `false` | Back to Auto every day at midnight |
| `auto_return_min` | 10 … 1440, or empty | Back to Auto this long after the last manual change; empty = stay manual |
| `fallback_level` | 0 … 4 | Level in Auto while the clock is not set |

## `[profile1]` … `[profile5]`

One line per weekday (`mon` … `sun`): switching times and levels, e.g.

```ini
mon = 00:00=1, 06:30=3, 08:00=1, 16:00=2, 22:30=1
```

Each slot (`HH:MM=level`) lasts until the next one; before a day's first slot, the previous
day's last level runs. Times must be sorted and unique.

## `[device]`

Taken over from the stock firmware by the import tool (`model`, `article`, `serial`,
`production_code`, `security_code`, `mac`). Read-only in the web UI; leave it as it is.
