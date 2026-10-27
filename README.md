# Heim

**A quiet, local home automation box.** Heim runs on a Raspberry Pi Zero 2 W or a Pi 3: a few
watts and about 10 MB of memory. It needs no cloud and no YAML: everything is set up from a phone.

[Français](README.fr.md) · [API](docs/API.md) · [Security](SECURITY.md) · [Translating](docs/TRANSLATING.md)

<p align="center">
  <img src="docs/home.png" width="230" alt="Home screen with tiles">
  <img src="docs/home-dark.png" width="230" alt="The same home screen in dark mode">
  <img src="docs/tile-editor.png" width="230" alt="Tile settings: function and appearance">
</p>

## Why Heim

- **Local.** Your devices, your rules, your logbook stay in the house. Heim works without the
  internet; nothing goes through someone else's server.
- **Frugal.** One small program written in Rust. About 10 MB of memory at rest, a database that
  spares the SD card, and enough room on a Pi Zero 2 W (512 MB).
- **Simple.** No configuration files: devices, rooms, scenes and automations are all set up from
  the interface, by anyone in the household.
- **Sturdy.** Every connection is isolated and restarted if it fails, a systemd watchdog restarts
  Heim if it ever freezes, and updates roll back on their own if a new version doesn't start.

## What it does

### Home screen
- **Several boards**, switched by swiping (little dots at the bottom) or with tabs.
- **Tiles from 1×1 to 4×4** on a grid that adapts to the screen: icon, name, state, and when
  there's room a brightness slider, a 24-hour graph or the device's other readings. Move them
  with a long press, resize them with the handle.
- **Each tile has two tabs.** *Function*: short, long and double press. *Appearance*: size,
  layout, a colour for on and one for off (a scene's colour, or any colour), fill, and colours
  by value (“above 25 °C: orange, *Hot*”).
- **Adding a tile** is quick: a search, and round filter buttons for lights, plugs, sensors and
  everything else.
- **House tiles** that update by themselves:
  - all the lights, all the plugs, a room;
  - *Leaving*: everything off, and the alarm armed if you wish;
  - who's home, alarm, triggered automations;
  - low batteries, unreachable devices;
  - sun, clock, scenes, timers.

### Devices and connections
- **Zigbee** through Zigbee2MQTT (Sonoff ZBDongle-E or similar). Pairing from the app, live.
- **Tuya Wi-Fi** plugs, bulbs and switches (LSC, Lidl…), controlled locally, without the cloud
  (protocols 3.3, 3.4 and 3.5).
- **Matter** through a Matter server (Home Assistant's, or python-matter-server).
- Every setting a device offers (colour, white, effects, fades, power-on behaviour, child
  lock…), firmware updates, battery and signal at a glance.

### Scenes and automations
- **Scenes**: each device set the way you want, with a live preview on the real lamps,
  templates, fades, and “on for 2 h”.
- **Automations**: *When* (a device changes, a threshold, a time, sunrise or sunset computed
  locally) / *If* / *Then* (control, turn on for a while, scene, notify, wait). Each one carries
  a true/false state: “mail arrived” stays raised until it's re-armed.

### Home and family
- **Alarm**: Away and Night modes, entry and exit delays, a check before arming, 24/7 sensors
  (smoke, leak), Zigbee sirens, lights, camera snapshots, optional code.
- **Presence** from phones on the Wi-Fi, with no GPS and no app.
- **Accounts**: admin, member, guest. Per person, you can set:
  - a forced or free board;
  - whether they may edit or add tiles;
  - which tabs they see;
  - read-only devices.

  The Pi itself enforces these rights.
- **Screens and wall tablets**: every phone or tablet that opens Heim shows up in Settings,
  Screens. From your phone, you can give it:
  - a board and a lock;
  - full-screen wall tablet mode;
  - keep-awake, and a return to its board after a few minutes.

  The tablet follows live.
- **Notifications** on the iPhone and Android, even with Heim closed (Web Push, no third-party
  service), plus web services (ntfy, Gotify…).
- **Remote access** with Tailscale: one https address at home and outside, no port opened on the
  router.

### Looks and languages
- Light, dark or the device's theme. Interface colour, text size, corners, spacing and background.
- **Languages**: French and English. The interface, server messages and notifications all
  follow each person's language.

<p align="center">
  <img src="docs/wall-tablet.png" width="520" alt="Wall tablet mode">
</p>

## Hardware

| | |
| --- | --- |
| Box | Raspberry Pi Zero 2 W, Pi 3, Pi 4 or Pi 5 (64-bit). About 1 to 2 W at rest for a Zero 2 W or a Pi 3. |
| Zigbee | A Zigbee USB stick and Zigbee2MQTT with its MQTT broker (for now, those of an existing Home Assistant are fine). |
| Storage | An 8 GB SD card or more. |

## Install

**With the SD image** (the simplest):

1. From the [releases](../../releases), download `heim-vX.Y.Z-heim.img.xz`.
2. In Raspberry Pi Imager: *Choose OS* › *Use custom*, then your SD card. Decline the “OS
   customisation”: Heim handles it.
3. Put the card in the Pi and power it. A minute or two later a **Heim-XXXX** Wi-Fi appears:
   join it with your phone and the setup opens by itself (otherwise open http://10.42.0.1).

<p align="center"><img src="docs/setup.png" width="560" alt="First start setup"></p>

The setup works like an installer, step by step:

| Step | |
| --- | --- |
| Language | Required. |
| Account | Required: Heim never runs without an administrator. |
| Network | Required. Cable or Wi-Fi, hidden network, fixed address, name of the box (`heim.local`). |
| Appearance | Optional. Theme, colour, text size… |
| Home | Optional. Rooms, location for sunrise and sunset, time zone. |
| Summary | Then into Heim. |

After switching to your home Wi-Fi, reconnect the phone to it, open **http://heim.local**, sign
in, and the setup carries on where it left off.

**On an existing Raspberry Pi OS Lite** (Bookworm):

```sh
curl -fsSL https://raw.githubusercontent.com/OWNER/heim/main/packaging/install.sh | sudo HEIM_REPO=OWNER/heim bash
```

**On a PC**, to try it with virtual devices:

```sh
cargo build --release
HEIM_DEMO=1 ./target/release/heim      # then http://<ip>:8090
```

## Updates

New versions are offered in Settings › System (checked twice a day). Heim checks the SHA-256,
backs up its data, and goes back to the previous version on its own if the new one doesn't
start. A backup is also made every night (the last 7 are kept).

## Configuration

Everything is set up in the interface. A few environment variables exist for special cases:

| Variable | Default | Role |
| --- | --- | --- |
| `HEIM_HTTP` | `0.0.0.0:8090` | Listening address (8090 to live alongside Home Assistant on 8123) |
| `HEIM_DATA_DIR` | `./data` | Folder of the `heim.db` database |
| `HEIM_TOKEN` | none | Technical token for scripts (admin rights). People use their accounts. |
| `HEIM_MQTT_HOST` / `_PORT` / `_USER` / `_PASSWORD` | none / `1883` | Zigbee2MQTT on the first start (then set in the interface) |
| `HEIM_Z2M_TOPIC` | `zigbee2mqtt` | Zigbee2MQTT base topic |
| `HEIM_DEMO` | `0` | `1` for virtual devices |
| `HEIM_NETSETUP` | `0` | `1`: Heim manages the Pi's network (NetworkManager), set by the SD image |
| `HEIM_HTTP_ALSO` | none | Extra listening addresses, comma-separated |
| `RUST_LOG` | `heim=info,warn` | `heim=debug` to see everything |

## Development

```text
src/            the core (Rust): registry, scenes, automations, alarm, accounts, API, i18n…
src/integrations/  zigbee2mqtt, tuya (+ tuya_proto), matter, virtual devices
ui/             the interface (Svelte 5), compiled into a single static/index.html
packaging/      SD image (pi-gen), systemd service, rollback guard, install.sh
tests/          end-to-end tests (virtual devices, alarm, accounts, Tuya, Matter)
docs/           API, translating, screenshots
```

```sh
cargo test                        # the core
cd ui && npm install && npm test  # the interface (and the translation check)
npm run dev                       # interface on :5173, wired to a Heim running on :8090
npm run build                     # regenerates static/index.html, then `cargo build`
```

The interface is embedded in the binary: Heim needs neither Node nor the internet to run.
To publish a version, bump `version` in `Cargo.toml`, push a `vX.Y.Z` tag: GitHub Actions builds
aarch64, armv7, x86_64 and Windows, then the SD image.

Want Heim in your language? See [docs/TRANSLATING.md](docs/TRANSLATING.md): one file to translate.

## Licence

Apache-2.0. Zigbee2MQTT (GPL-3.0) is used as a separate service, over MQTT: none of its code is included.
