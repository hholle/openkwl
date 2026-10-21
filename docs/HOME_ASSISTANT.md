# Home Assistant

openKWL talks to Home Assistant over **MQTT with auto-discovery**. Once it is connected, the
unit shows up as a device with 29 entities: no YAML and no custom integration needed.

## Set it up

You need an MQTT broker that Home Assistant uses, for example the
[Mosquitto add-on](https://github.com/home-assistant/addons/blob/master/mosquitto/DOCS.md)
with the [MQTT integration](https://www.home-assistant.io/integrations/mqtt/).

1. In Home Assistant, create a user for the unit (Mosquitto add-on: *Settings › People › Users*,
   or the add-on's `logins` option).
2. In the openKWL web UI, open *Settings › Home Assistant*:
   - turn on **Connect to Home Assistant (MQTT)**;
   - **Broker:** the broker's IP address or DNS name. `.local` names don't work; the unit has
     no mDNS resolver.
   - **Port:** usually `1883`;
   - **User name** and **Password** of the MQTT user.
3. Save. Within a few seconds the device appears in Home Assistant under *Settings › Devices &
   services › MQTT*, named after the unit's host name (default `openkwl`). Its model is the one
   the unit reports (`model` in `CONFIG.INI`, taken over from the stock firmware by the import).

Under *Show advanced settings* you can change:

| Setting | Default | Notes |
|---|---|---|
| Client ID | `openkwl` | Also the device's node ID in discovery topics; must be unique on the broker. Letters, digits, `-`, `_`. |
| Base topic | `openkwl` | State and command topics live below it. Units set up before this default keep their saved topic (e.g. `helios/kwl`) |
| Discovery | on | Turn off if you want to define the entities yourself |
| Discovery prefix | `homeassistant` | Must match the MQTT integration's discovery prefix |

When two units share one broker, give each its own client ID and base topic.

## Entities

Entity IDs follow the device name, for example `fan.openkwl` and
`sensor.openkwl_outdoor_air`.

### Control

| Entity | Type | Notes |
|---|---|---|
| *(device name)* | `fan` | On/off, speed in 4 steps (levels 1–4), presets `auto`, `party`, `quiet`. Setting a speed switches to manual; during Party or Quiet it changes that program's level instead. Turning it off stops both fans (level 0). While a holiday runs, speed, on/off and presets are ignored and the entity snaps back: end the holiday in the web UI first. |
| Mode | `sensor` (enum) | `Manual`, `Auto`, `Party`, `Quiet`, `Holiday`, the same names as in the web UI |
| Returns to Auto in | `sensor` (duration, min) | Time left until manual mode returns to Auto, if *Back to Auto* is set |

### Air and heat recovery

| Entity | Type | Unit |
|---|---|---|
| Outdoor air, Supply air, Extract air, Exhaust air | `sensor` (temperature) | °C |
| Heat recovery efficiency | `sensor` | % |
| Supply air flow, Extract air flow | `sensor` (volume flow rate) | m³/h, estimated ([AIRFLOW.md](AIRFLOW.md)) |
| Recovered heat power | `sensor` (power) | W |
| Recovered heat, Recovered cooling | `sensor` (energy, `total_increasing`) | kWh; usable in the Energy dashboard and long-term statistics |

### Filter, bypass, frost, faults

| Entity | Type | Notes |
|---|---|---|
| Filter change in | `sensor` (duration, d) | Days of fan running time left |
| Filter change due | `binary_sensor` (problem) | |
| Reset filter interval | `button` (config) | After you've changed the filters |
| Summer bypass | `binary_sensor` (opening) | Whether the bypass is open now |
| Bypass cooling | `switch` (config) | Allows the bypass to open for cooling |
| Bypass target temperature | `number` (config) | 10–35 °C in 0.5 steps |
| Frost protection | `binary_sensor` (cold) | Frost protection is active |
| Defrosting | `binary_sensor` (running) | A defrost cycle is running |
| Fault | `binary_sensor` (problem) | On for a real fault; attribute `faults` lists them all |

### Diagnostics

Frost protection state (enum), supply/exhaust fan speed (rpm), supply/exhaust fan voltage (V),
operating hours, and a **Restart** button.

### Faults

The `faults` attribute of *Fault* (and `faults` in `GET /api/v1/status`) can contain:

| Fault | Meaning |
|---|---|
| `supply_fan_stall`, `exhaust_fan_stall` | The fan should turn but reports no speed |
| `<sensor>_sensor_open`, `<sensor>_sensor_short` | A temperature sensor is disconnected or shorted (`outdoor`, `supply`, `extract`, `exhaust`) |
| `sd_card` | The SD card is missing or failing |
| `config_invalid` | `CONFIG.INI` failed to load; the last good copy is in use |
| `bus_overcurrent` | The module bus supply was switched off after repeated overcurrent (KNX module unpowered) |
| `filter_change`, `time_not_set` | Notices only; they don't turn *Fault* on |

## Updates and availability

- **Switching values arrive within about a second:** mode, level, preset, bypass, frost,
  faults, filter.
- **Measurements are published when they change noticeably,** and at most every 30 s.
  "Noticeably" means 0.2 K, 1 %, 20 W, 0.05 kWh or 5 m³/h. Fan speed and fan voltage, which are
  diagnostics, go at most every 60 s. Home Assistant records every state change and keeps
  5-minute statistics, so faster updates would only fill its database.
- **Everything is republished every 5 minutes,** which also keeps the energy counters current
  for the Energy dashboard.
- **Availability:** `online`/`offline` on `<base>/availability`. A last will marks the unit
  offline if it drops off the network.
- When Home Assistant restarts (it publishes `online` on `homeassistant/status`), the unit sends
  its discovery messages again.

## Automation examples

Boost the ventilation while someone showers (a humidity sensor in the bathroom):

```yaml
automation:
  - alias: "Ventilation: party while showering"
    triggers:
      - trigger: numeric_state
        entity_id: sensor.bathroom_humidity
        above: 70
    actions:
      - action: fan.set_preset_mode
        target: { entity_id: fan.openkwl }
        data: { preset_mode: party }
```

Quiet at night when everyone is in bed, and back to the weekly program in the morning:

```yaml
automation:
  - alias: "Ventilation: quiet at bedtime"
    triggers:
      - trigger: state
        entity_id: input_boolean.bedtime
        to: "on"
    actions:
      - action: fan.set_preset_mode
        target: { entity_id: fan.openkwl }
        data: { preset_mode: quiet }
  - alias: "Ventilation: auto in the morning"
    triggers:
      - trigger: time
        at: "06:30:00"
    actions:
      - action: fan.set_preset_mode
        target: { entity_id: fan.openkwl }
        data: { preset_mode: auto }
```

Get a notification when the filter is due or a fault appears:

```yaml
automation:
  - alias: "Ventilation: needs attention"
    triggers:
      - trigger: state
        entity_id:
          - binary_sensor.openkwl_filter_change_due
          - binary_sensor.openkwl_fault
        to: "on"
    actions:
      - action: notify.notify
        data:
          message: >-
            Ventilation: {{ state_attr('binary_sensor.openkwl_fault', 'faults') | default('filter change due') }}
```

Party and Quiet run for the time set under *Schedule › Party and Quiet*, then return by
themselves. The weekly program and the holiday dates are set in the web UI; automations
switch between the modes, except during a holiday, which ignores them until it is ended.

Each mode or speed change is saved on the SD card about a minute later, so a restart keeps
it; an automation that changes the speed many times a day is saved at once only for the first
few changes a day, after that with the regular save every 6 hours (a power cut saves too).

> [!CAUTION]
> The *Bypass cooling* switch turns bypass cooling on without the web UI's reminder: as the
> stock UI warns, the unit's rubber plug (condensate) must be removed before cooling is used.
> Check your unit's manual before you automate it.

## MQTT topics

For other MQTT clients (Node-RED, openHAB, scripts). `<base>` is the base topic (default
`openkwl`).

| Topic | Direction | Payload |
|---|---|---|
| `<base>/availability` | unit → broker, retained, last will | `online` / `offline` |
| `<base>/state` | unit → broker, retained | JSON, see below |
| `<base>/fan/set` | broker → unit | `ON` / `OFF` |
| `<base>/fan/level/set` | broker → unit | `0`–`4` |
| `<base>/fan/preset/set` | broker → unit | `auto` / `party` / `quiet` / `None` (manual) |
| `<base>/bypass/enabled/set` | broker → unit | `ON` / `OFF` |
| `<base>/bypass/target/set` | broker → unit | °C, e.g. `22.5` |
| `<base>/filter/reset` | broker → unit | `PRESS` |
| `<base>/system/reboot` | broker → unit | `PRESS` |

Discovery messages are retained on `<prefix>/<component>/<client id>/<object id>/config`, one
per entity.

The state document (missing readings are `null`):

```json
{
  "fan": "ON", "level": 2, "preset": "auto", "mode": "auto", "auto_return_min": null,
  "t_outdoor": 13.7, "t_supply": 20.6, "t_extract": 22.1, "t_exhaust": 15.8,
  "efficiency": 82.4, "heat_w": 307.0, "heat_kwh": 412.35, "cool_kwh": 18.2,
  "supply_rpm": 1350, "exhaust_rpm": 1340, "supply_m3h": 159.0, "exhaust_m3h": 158.0,
  "supply_v": 6.0, "exhaust_v": 6.0,
  "bypass": "OFF", "bypass_enabled": "ON", "bypass_target": 22.0,
  "frost": "OFF", "defrost": "OFF", "frost_state": "idle",
  "filter_days": 50, "filter_due": "OFF", "op_hours": 18234,
  "problem": "OFF", "faults": []
}
```

The implementation and its tests are in [`crates/openkwl-ha`](../crates/openkwl-ha/src/lib.rs).

## Security

MQTT runs without TLS: the password and every command cross the network in clear text. Anyone
who can publish to `<base>/…/set` controls the unit. Give the unit its own broker user, and
limit what it may read and write with the broker's access rules. See
[SECURITY.md](../SECURITY.md).

## Troubleshooting

| Problem | Check |
|---|---|
| Device doesn't appear | *Settings › Technical details* shows the MQTT state. Check broker address and port, the user and password, and that the discovery prefix matches Home Assistant's. |
| Entities are "unavailable" | The unit is offline, or the broker lost it: check `<base>/availability` with *MQTT › Listen to a topic*. |
| Duplicate or stale entities after renaming | Delete the old device under *Devices & services › MQTT*; retained discovery messages can be cleared with an empty retained message on their topic. |
| Air flow looks wrong | It's an estimate; calibrate it under *Settings › Fan levels* ([AIRFLOW.md](AIRFLOW.md)). |
