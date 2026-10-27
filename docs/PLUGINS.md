# Writing a Heim plugin

A plugin is a small program that runs next to Heim on the box. Heim downloads it, checks its
SHA-256, starts it with its settings, restarts it if it stops, and shows its memory use. The
plugin talks to Heim through the MQTT broker, using the **Home Assistant MQTT discovery**
format: anything it announces shows up in Heim as a device (tiles, scenes, automations).

## 1. The manifest: `heim-plugin.json`

Put it at the root of a public GitHub repository. People install the plugin by typing
`owner/repo` in Settings › Connections › Store › *Add an external plugin*.

```json
{
  "id": "wled",
  "name": "WLED",
  "version": "1.0.0",
  "category": "connections",
  "author": "Alex",
  "icon": "bulb",
  "description": { "en": "WLED LED strips.", "fr": "Rubans LED WLED." },
  "memory_mb": 12,
  "permissions": ["network", "mqtt"],
  "homepage": "https://github.com/alex/heim-wled",
  "binaries": {
    "aarch64-linux":  { "url": "https://github.com/alex/heim-wled/releases/download/v1.0.0/wled-aarch64-linux", "sha256": "…" },
    "armv7-linux":    { "url": "…", "sha256": "…" },
    "x86_64-linux":   { "url": "…", "sha256": "…" },
    "x86_64-windows": { "url": "…", "sha256": "…" }
  },
  "settings": [
    { "key": "host", "type": "string", "label": { "en": "Strip address", "fr": "Adresse du ruban" }, "required": true },
    { "key": "port", "type": "number", "default": 80, "label": "Port" },
    { "key": "token", "type": "password", "label": "Token" },
    { "key": "mode", "type": "select", "options": ["fast", "quiet"], "default": "quiet", "label": "Mode" }
  ]
}
```

| Field | |
| --- | --- |
| `id` | `a-z`, `0-9`, `-`, `_`, 2 to 40 characters. Must not be a built-in id (`zigbee2mqtt`, `tuya`, `matter`, `mqtt_discovery`). |
| `category` | `connections`, `display`, `features` or `network`. |
| `memory_mb` | What it usually takes. Shown before installing. Be honest: many boxes are a Pi Zero 2 W. |
| `binaries` | One program per machine, with its SHA-256. https only. A machine without an entry can't install the plugin. |
| `settings` | Turned into a form by Heim. Types: `string`, `password` (never shown back), `number`, `boolean`, `select`. |
| `permissions` | `network`, `usb`, `mqtt`, `bluetooth`: shown in the warning, for now informative. |

## 2. What the plugin receives

Environment variables:

| Variable | |
| --- | --- |
| `HEIM_PLUGIN_ID` | Its id. |
| `HEIM_PLUGIN_SETTINGS` | Its settings, as one JSON object. |
| `HEIM_PLUGIN_DATA` | A folder of its own (also the working directory). |
| `HEIM_MQTT_HOST`, `HEIM_MQTT_PORT` | The broker. On the box itself, no account is needed. |
| `HEIM_MQTT_USERNAME`, `HEIM_MQTT_PASSWORD` | Only when Heim reads an external broker. |
| `HEIM_DISCOVERY_PREFIX` | `homeassistant`. |
| `HEIM_LANG` | `fr` or `en`. |

Whatever it prints (stdout, stderr) appears in its log in Heim.

## 3. Announcing devices

Publish a retained message on `homeassistant/<type>/<node>/<object>/config`, then the state on
the topic you announced. Heim understands `light` (default and `json` schemas, brightness),
`switch`, `sensor` and `binary_sensor`, with `availability`, `device`, `device_class`,
`unit_of_measurement` and simple `value_template`s (`{{ value_json.a.b }}`, `| float`,
`| round(1)`). Abbreviations (`stat_t`, `cmd_t`, `~`…) work.

```json
// homeassistant/sensor/wled_salon/power/config
{
  "name": "Power", "uniq_id": "wled_salon_power", "dev_cla": "power", "unit_of_meas": "W",
  "stat_t": "wled/salon/power", "dev": { "ids": ["wled_salon"], "name": "WLED salon" }
}
```

Commands arrive on the `command_topic` you announced (`ON`/`OFF`, or JSON for the `json`
light schema). To remove a device, publish an empty retained message on its `config` topic.

## 4. Getting into the official store

Official plugins are listed in [`plugins/store.json`](../plugins/store.json) with a
`manifest_url`. Open a pull request adding yours; it is reviewed before it gets the
*official* badge. Until then, people can still install it from its repository, after a
warning that the developer isn't certified.
