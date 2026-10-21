# Documentation

## For homeowners

| Document | Read it to … |
|---|---|
| [User guide](USER_GUIDE.md) | use the web UI: modes, schedule, holiday, settings |
| [Installation](INSTALL.md) | try openKWL with a second SD card, and go back |
| [Updating](UPDATING.md) | install new versions from the web UI; how the update file is checked |
| [Home Assistant](HOME_ASSISTANT.md) | connect the unit, the entities, automation examples, MQTT topics |
| [FAQ and troubleshooting](FAQ.md) | find quick answers, and what to do when something is wrong |
| [Air flow](AIRFLOW.md) | understand and calibrate the m³/h estimate |
| [`CONFIG.INI` reference](CONFIG_INI.md) | edit the settings file by hand: every key and value |
| [Security](../SECURITY.md) | know the security model and its limits; report a vulnerability |

## For integrators

| Document | Read it to … |
|---|---|
| [REST API guide](API.md) | log in, read values, control the unit, with `curl` examples |
| [OpenAPI contract](../api/openapi.yaml) | see every endpoint, field and range |
| [MQTT topics](HOME_ASSISTANT.md#mqtt-topics) | use the unit from Node-RED, openHAB or scripts |
| [Update package format](UPDATING.md#the-okwl-package-format) | build or inspect `.okwl` files |

## For contributors

| Document | Read it to … |
|---|---|
| [Contributing](../CONTRIBUTING.md) | report bugs, test reports and pull requests |
| [Development guide](DEVELOPMENT.md) | build, run the simulator, test, release |
| [Architecture](ARCHITECTURE.md) | understand the crates, the runtime and the design decisions |
| [Firmware internals](../firmware/README.md) | build variants, debugging on the unit, watchdog, memory budgets |
| [Roadmap and open work](PLAN.md) | see what's open, what to confirm on a unit, and the design notes |
| [Agent and project rules](../AGENTS.md) | follow the conventions (for people and coding agents) |
| [Vendored crates](../vendor/README.md) | what the `serde-json-core` patch changes, and why |
| [Changelog](../CHANGELOG.md) | see what changed in each version |
