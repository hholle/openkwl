# Air flow without a flow sensor

The KWL EC 300 W has no flow sensor, and the stock firmware shows no m³/h for it: Helios
classes the KWL EC 200/300/500 W as *drehzahlkonstant* (constant speed). At commissioning the
installer reads the flow off printed curves (manual 82237, §3.1). openKWL estimates it from
the measured fan speed instead, with `openkwl_core::airflow`.

## Method

The EC fans (ebm-papst R3G146-AD, DIBt approval Z-51.3-438) hold their speed. In a fixed duct
system (p = c·Q²) the fan affinity laws then give **Q = k · n**. `k` depends on your ducts:

| Duct system | k (m³/h per 1000 rpm) | Level 2 (6 V, 1350 rpm) |
|---|---|---|
| DIBt reference: 100 Pa at 280 m³/h (default) | **118** | ≈ 159 m³/h |
| Stiff ducts: 100 Pa at 150 m³/h | 93 | ≈ 126 m³/h |

The default 118 intersects the 10 V catalogue curve with the DIBt reference system, at about
2370 rpm at 10 V. That speed is extrapolated from the owner's unit (1350 rpm at 6 V) and has not
been measured.

**Accuracy:** about ±15–25 % without calibration and about ±10 % with one measurement. A
clogging filter shifts the system curve, so the error grows until the filter is changed.

**Calibrating:** measure the air flow once at any level, with an anemometer at a valve or the
installer's commissioning protocol. Then either
- enter the measured value per level under *Settings → Fans* (measured values always win), or
- set *Air flow per 1000 rpm* to `measured m³/h ÷ rpm × 1000`.

The estimate feeds the overview, the heat-recovery power and energy, and the Home Assistant
sensors `supply_airflow` / `exhaust_airflow` (`volume_flow_rate`, m³/h).

## Catalogue curves (KWL EC 300 W, supply)

Digitised from the Helios catalogue page (the same curves are in the commissioning manual §4.1).
The reading error is about ±5 m³/h. A dash means the curve ends before that pressure.

| Control voltage | 0 Pa | 50 Pa | 100 Pa | 150 Pa | 200 Pa |
|---|---|---|---|---|---|
| 3 V | 66 | – | – | – | – |
| 4 V | 101 | 41 | – | – | – |
| 5 V | 138 | 92 | 22 | – | – |
| 6 V | 178 | 140 | 96 | 26 | – |
| 7 V | 217 | 187 | 153 | 112 | 53 |
| 8 V | 246 | 225 | 200 | 171 | 134 |
| 9 V | 279 | 258 | 238 | 216 | 192 |
| 10 V | 314 | 297 | 279 | 260 | 239 |

Sources:
- Helios catalogue page KWL EC 300 W: 315 m³/h at 0 Pa / 10 V, 2 × 100 W.
- ErP fiche: maximum 280 m³/h at 158 W; SEL 0.375 W/(m³/h) at 189 m³/h and 50 Pa.
- DIBt approval Z-51.3-438, annex 5: 332 / 280 / 216 / 143 m³/h at 0 / 100 / 200 / 300 Pa.
- Helios *KWL easyControls Inbetriebnahme* 82237 (in `KWL_EC_200W_300W.zip` from heliosselect.de).
