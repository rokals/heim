# Heim API

REST under `/api`, JSON everywhere. Errors look like `{"error": "readable message"}`, in the language sent in the `X-Heim-Lang` header (`fr`, `en`), otherwise the browser's or the house's.

```text
GET    /api/health
GET    /api/entities
GET    /api/entities/{id}
PATCH  /api/entities/{id}                {"name": "...", "room": "Salon", "plan": {"x": 0.3, "y": 0.6}}
POST   /api/entities/{id}/command        {"action": "turn_on", "brightness": 180} | {"action": "toggle"}

GET    /api/scenes
PUT    /api/scenes/{id}                  {"name": "Soirée", "targets": {"light.salon": {"on": true, "brightness": 90, "color_temp": 370}},
                                          "transition_s": 3, "off_mode": "turn_off" | "restore", "icon": "sofa", "color": "#f76b15"}
POST   /api/scenes/{id}/capture          {"name": "Lecture", "entities": ["light.salon", "light.lampadaire"]}
POST   /api/scenes/{id}/activate     {"minutes": 120} (optional: on for a while)
POST   /api/scenes/{id}/deactivate | /duplicate
POST   /api/scenes/snapshot          {"entities": ["light.salon"]}   current state, as scene settings
POST   /api/scenes/preview           {"targets": {"light.salon": {"on": true, "color": "#ff8800"}}}   live preview
POST   /api/scenes/preview/end       everything goes back as before
DELETE /api/scenes/{id}

GET    /api/timers
PUT    /api/timers/{id}                  {"name": "Cafetière", "duration_s": 1800, "on_finish": {"entity_id": "switch.prise_cafe", "action": "turn_off"}}
POST   /api/timers/{id}/control          {"op": "start"} | {"op": "start", "duration_s": 600} | {"op": "pause"} | {"op": "resume"} | {"op": "cancel"}
DELETE /api/timers/{id}

POST   /api/entities/{id}/turn_on_for    {"minutes": 30}
POST   /api/rooms/rename                 {"from": "Salon", "to": "Séjour"}

GET    /api/devices/{id}/controls        device settings (id = Zigbee IEEE address or tuya:<id>)
POST   /api/devices/{id}/set             {"values": {"color": {"hex": "#ff8800"}, "transition": 2}}
POST   /api/devices/{id}/action          {"action": "ota_update"} | ota_check | interview | {"action": "remove", "force": true}
GET    /api/power_on
PUT    /api/power_on                     {"behavior": "previous", "apply_new": true}   previous | on | off
POST   /api/automations/{id}/op       {"state": "rearm"} | {"state": true} | {"enabled": false} | {"run": true}
GET    /api/logbook?entity_id=&kind=&q=&before=&limit=   logbook, most recent first
GET    /api/history/{entity_id}?hours=24&points=200      sensor graph
GET    /api/groups  |  PUT /api/groups   [{"name": "Tout le salon", "room": "Salon", "members": ["light.a", "light.b"]}]
POST   /api/entities/{id}/battery_reset  battery changed
GET    /api/backups  |  POST /api/backups  |  GET /api/backups/{nom}   nightly backups
POST   /api/system/reboot  |  POST /api/system/poweroff
GET    /api/dashboards | PUT /api/dashboards {"boards": [...]} | PUT /api/dashboards/{id} {"sections": [...]}
GET    /api/screens | PUT /api/screens/{id} | DELETE /api/screens/{id} | POST /api/screens/{id}/identify
GET    /api/appearance | PUT /api/appearance | PUT /api/appearance/me
PUT    /api/users/{id}/access            {"board": "kitchen", "can_edit": false, "can_add": true, "hidden_tabs": [], "readonly": []}
GET    /api/sun
GET    /api/integrations/catalog
PUT    /api/integrations/{nom}/enabled   {"enabled": false}

GET    /api/automations
PUT    /api/automations/{id}             {"name": "...", "triggers": [...], "conditions": [...], "actions": [...]}
POST   /api/automations/{id}/run
DELETE /api/automations/{id}

GET    /api/settings/{clé}               zigbee2mqtt | home | location | alerts | ui.*
PUT    /api/settings/{clé}
GET    /api/system
GET    /api/backup                       a .db file
POST   /api/restore                      a .db file, loaded at the next start

GET    /api/security                     state, settings (without the code), history, photos
PUT    /api/security/config              {"sensors": [{"entity_id": "binary_sensor.porte", "role": "perimeter", "entry": true}], "sirens": [...], "lights": [...], "cameras": [{"name": "Entrée", "snapshot_url": "http://..."}], "exit_delay_s": 60, "entry_delay_s": 30}
PUT    /api/security/code                {"current": "1234", "new": "5678"}   (empty new = remove)
POST   /api/security/arm                 {"mode": "away" | "night", "force": false, "code": null}   409 if open / unreachable
POST   /api/security/disarm              {"code": "1234"}
POST   /api/security/test_sirens         {"seconds": 3}
POST   /api/security/snapshot            {"camera": "Entrée"}
GET    /api/security/snapshots/{nom}     JPEG image

GET    /api/zigbee/pairing               {"open": true, "until": "...", "version": "2.1.0", "events": [...]}
POST   /api/zigbee/permit_join           {"seconds": 240}   (0 = close)

GET    /api/auth/status                  public: {"open": false, "users": [{"id": "alex", "name": "Alex"}]}
POST   /api/auth/login                   {"name": "alex", "password": "..."}   sets the session cookie
POST   /api/auth/logout | /api/auth/password {"current": "...", "new": "..."}
GET    /api/auth/me
GET    /api/users | POST /api/users      {"name": "Camille", "role": "member", "password": "..."}
PUT    /api/users/{id}                   {"role": "guest"} | {"password": "..."}
DELETE /api/users/{id}

GET    /api/push/key                     VAPID public key
POST   /api/push/subscribe               {"target_id": "app_x1y2z3", "subscription": {...}}   (DELETE to unsubscribe)
POST   /api/push/test                    {"target_id": "app_x1y2z3"}

GET    /api/presence | PUT /api/presence/config | POST /api/presence/scan
GET    /api/remote                       Tailscale detected? https address?

GET    /api/setup                        public: setup needed? network?
POST   /api/setup/scan | /api/setup/wifi {"ssid", "password", "hidden", "static": {"address", "gateway", "dns"}} | /api/setup/ethernet
POST   /api/setup/step {"step": "look"} | /api/setup/hostname {"name"} | /api/setup/timezone {"timezone"} | /api/setup/done
GET    /api/update | POST /api/update/check | POST /api/update/install | PUT /api/update/settings {"repo": "..."}
POST   /api/system/restart | /api/system/reboot

GET    /api/tuya | PUT /api/tuya         {"enabled": true, "devices": [{"id", "name", "key", "ip", "version"}]}
POST   /api/tuya/import                  contents of a tinytuya devices.json
POST   /api/tuya/scan                    devices announcing themselves on the network
GET    /api/matter | PUT /api/matter     {"enabled": true, "url": "192.168.1.50:5580"}
POST   /api/matter/commission            {"code": "MT:..."}
POST   /api/matter/remove                {"node_id": 4}

POST   /api/notify                       {"title": "Maison", "message": "Linge terminé", "to": ["person:alex"], "except": ["ecran_cuisine"]}
GET    /api/notify/targets
PUT    /api/notify/targets/{id}          {"name": "ntfy", "kind": "webhook", "url": "https://ntfy.sh/mon-sujet"}
GET    /api/notify/inbox/{target}?after={id}
```

WebSocket `/api/ws`: a `snapshot` message on connection, then events (`state_changed`,
`entity_updated`, `entity_removed`, `command_timeout`, `integration_status`, `notification`,
`security`, `zigbee_bridge`, `zigbee_device`).
The client can send:

```json
{"type": "hello", "target_id": "tel_alex", "name": "Alex's phone", "lang": "en"}
{"type": "command", "entity_id": "light.salon", "action": "toggle"}
```

