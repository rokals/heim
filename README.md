# Heim

Noyau domotique léger, écrit en Rust. Conçu pour une maison simple (quelques ampoules, prises et
capteurs) qui doit marcher sans YAML et sans planter, sur un Raspberry Pi Zero 2 W (512 Mo) ou un Pi 3.

> Statut : 0.6. Ouvrez `http://<ip>:8090` sur un téléphone ou un ordinateur : tout se règle dans l'interface.

## Ce que fait le noyau

| Fonction | Comment |
| --- | --- |
| Entités | Lumières, prises, capteurs, détecteurs, scènes, timers. État réel + état demandé. |
| Scènes | Chaque appareil réglé dans la scène : allumé ou éteint, luminosité, blanc, couleur, effet (seuls les réglages cochés comptent). Aperçu en direct pendant le réglage (tout revient comme avant à la sortie), fondu, modèles (Soirée, Lecture, Ciné, Nuit, Réveil), dupliquer, icône et couleur. La scène sait si elle est active (avec une marge, couleur comprise) ; la désactiver éteint tout ou revient comme avant, au choix ; « activer pour 2 h » revient tout seul. |
| Timers avec décompte | Heure de fin absolue : décompte fluide côté interface, survit aux redémarrages, action à l'échéance (couper une prise...). |
| Notifications | Chaque téléphone qui ouvre l'app devient une cible. « Tous sauf un » : `{"message": "...", "except": ["tel_alex"]}`. |
| Zigbee | Via Zigbee2MQTT (clé Sonoff ZBDongle-E ou autre). Découverte automatique des appareils. |
| Fiabilité | Intégrations isolées et relancées si elles plantent, reconnexion MQTT automatique, appareils marqués injoignables pendant une coupure, SQLite en WAL avec écritures groupées (ménage la carte SD). |
| Alarme | Modes Absent et Nuit, délais de sortie et d'entrée, vérification avant activation (« la fenêtre est ouverte »), capteurs 24 h/24 (fumée, fuite), capteur muet signalé, sirènes Zigbee, lumières, photo des caméras, code facultatif, historique. L'état survit aux redémarrages. |
| Appairage | « Ajouter un appareil » ouvre le réseau Zigbee quelques minutes et montre l'arrivée de l'appareil en direct (Zigbee2MQTT 1.x et 2.x). |
| Comptes | Admin (règle tout), Membre (utilise tout, alarme comprise), Invité (lumières, prises, scènes ; ni l'alarme ni les sirènes). Connexion en touchant son prénom. Mots de passe hachés (PBKDF2), sessions par cookie. Sans compte, Heim reste ouvert comme avant. |
| Notifications push | Web Push chiffré (RFC 8291, VAPID), sans service tiers : l'iPhone reçoit les alertes même Heim fermé (iOS 16.4+, Heim ajouté à l'écran d'accueil, adresse HTTPS). |
| Accès à distance | Guide Tailscale intégré (`tailscale serve`) : une adresse `https://…ts.net` valable à la maison et dehors, aucun port ouvert sur la box. |
| Présence | Téléphones vus sur le Wi-Fi (table ARP, ping) : entités `person.*` pour les automatismes, « tout le monde est parti » propose ou active l'alarme. Jamais de désactivation automatique. |
| Tuya local | Prises, ampoules, interrupteurs Wi-Fi (LSC Action, Lidl…), protocoles 3.3, 3.4 et 3.5, sans cloud. Import du `devices.json` de tinytuya, découverte réseau automatique. Vérifié contre la bibliothèque tinytuya. |
| Matter | Par un serveur Matter (celui de Home Assistant ou python-matter-server) : lumières, prises, capteurs, appairage par code de partage. |
| Mises à jour | Depuis les releases GitHub, en un clic dans Réglages > Système : contrôle SHA-256, sauvegarde de la base, retour automatique à l'ancienne version si la nouvelle ne démarre pas. |
| Image SD | Raspberry Pi OS Lite 64 bits + Heim, fabriquée par GitHub Actions. Premier démarrage : réseau Wi-Fi « Heim-XXXX » et assistant (compte, Wi-Fi). Ensuite http://heim.local. |
| Maison | Nom, étages, pièces (icône, ordre, étage). Les pièces servent partout : rangement des appareils, ordre de l'accueil, assistant du premier démarrage. |
| Réglages d'appareil | Tout ce que l'appareil accepte, lu dans ses `exposes` Zigbee2MQTT ou ses DP Tuya : couleur, luminosité, blanc, effets, fondu, état après coupure, LED, verrouillage enfant, niveaux de démarrage… Mise à jour OTA, nouvel interview, retrait du réseau. |
| Après une coupure | Un seul choix (Comme avant / Éteint / Allumé) appliqué à tous les appareils compatibles, et aux nouveaux dès leur ajout. |
| Boutons d'accueil | Tout peut aller sur l'accueil (appareil, groupe, scène, automatisme, personne), choisi dans une liste à onglets. Chaque bouton a son appui court et son appui long (basculer, allumer, réarmer, ouvrir le détail…) et ses règles d'apparence : « si température > 25 : orange, "Chaud" ». Appui long pour déplacer en mode modification. |
| État des automatismes | Chaque automatisme porte un vrai/faux (entité `automation.<id>`) : il passe à l'inverse de son état au repos quand il se lance, et revient au repos par un appui long, l'action « Réarmer », ses déclencheurs de réarmement (« la trappe arrière s'ouvre ») ou un délai. La boîte aux lettres, la lessive finie… |
| Pourquoi pas lancé | La dernière fois qu'un déclencheur a réagi : lancé, ou la condition qui a bloqué (« il ne faisait pas nuit »). |
| Journal et courbes | Tout ce qui se passe, et qui l'a fait (30 jours) ; courbes des capteurs (10 jours, un point / 5 min). Base séparée `history.db`, écrite par paquets de 30 s. |
| Groupes | Plusieurs lumières ou prises commandées comme une seule (`group.<id>`). |
| Pile changée | Efface l'ancien niveau, que Zigbee2MQTT renvoie de mémoire, jusqu'au vrai nouveau niveau ; « Relire le niveau » le demande à l'appareil. |
| Fiabilité | Chien de garde systemd (Heim figé = relancé), journaux système en mémoire (carte SD ménagée), alerte « Zigbee2MQTT injoignable depuis 5 min », sauvegarde automatique chaque nuit (7 gardées), arrêt et redémarrage propres depuis Réglages > Système, page grisée quand la connexion est perdue. |
| Plan de maison | Chaque entité peut porter une position `plan: {x, y, floor}` (l'interface glisser-déposer viendra avec le front). |

Mémoire mesurée : environ 10 Mo au repos (binaire x86_64, Zigbee2MQTT connecté, 7 entités).

## Interface

L'interface (dossier `ui/`, Svelte 5 + Vite) est compilée en **un seul fichier**
`static/index.html` (JS, CSS et police compris), embarqué dans le binaire. Heim n'a donc
besoin ni de Node ni d'Internet pour fonctionner : Node ne sert qu'à modifier l'interface.

Cinq onglets :

- **Accueil** : rangé tout seul par pièce, ou personnalisé (« Personnaliser l'accueil ») :
  sections, cartes déplacées au doigt, taille 1 ou 2 cases, « un clic allume pour 30 min ».
  Bouton « Éteindre/Allumer la pièce » ; les lumières allumées s'éclairent.
- **Carte d'alarme** en haut de l'accueil dès qu'elle est configurée : Absent, Nuit,
  décompte, alarme déclenchée en rouge avec la photo de la caméra, pavé numérique.
- **Appareils** : « Ajouter » pour appairer un appareil Zigbee ; tout sur une page, recherche, filtres (sans pièce, piles classées de la plus faible,
  signal Zigbee en barres, injoignables, masqués), « vu il y a… ». La fiche d'un appareil a trois onglets :
  Commandes (lampe : couleur, blanc, luminosité, effets, fondu), Réglages (nom, pièce, tous ses paramètres), Infos (modèle, firmware, mise à jour).
- **Scènes** : « Enregistrer cette ambiance ».
- **Automatismes** : Quand (changement d'état, depuis X min, seuil, heure, soleil) / Si
  (plage horaire, jours, nuit, état) / Alors (commander, allumer pour X min, scène,
  notifier, attendre). Modèles prêts à l'emploi.
- **Réglages** en rubriques : journal, comptes, présence, accès à distance, sécurité (rôle de chaque capteur, sirènes et essai 3 s,
  lumières, délais, caméras, code, historique), maison (étages et pièces), intégrations (Zigbee, Tuya, Matter : une carte chacune avec
  son état et son interrupteur, et l'état après coupure de courant pour tous les appareils),
  notifications (téléphones et services web), alertes (piles, appareils muets, fuites),
  lieu (soleil calculé sans Internet), accès, sauvegarde/restauration, système.

Modifier l'interface :

```sh
cd ui
npm install
npm run dev     # interface sur http://localhost:5173, branchée sur le Heim lancé à côté (port 8090)
npm test        # tests de la logique de regroupement
npm run build   # régénère static/index.html, puis `cargo build` pour l'embarquer
```

## Installer sur un Raspberry Pi (Pi 3, Pi 4, Pi 5, Pi Zero 2 W)

**Avec l'image SD** (le plus simple) :

1. Dans les [releases](../../releases), téléchargez `heim-vX.Y.Z-heim.img.xz`.
2. Raspberry Pi Imager : *Choisir l'OS* > *Utiliser une image personnalisée*, puis la carte SD.
   Refusez la « personnalisation de l'OS » : Heim s'en charge.
3. Carte dans le Pi, alimentation. Une à deux minutes plus tard apparaît le Wi-Fi
   **Heim-XXXX** (sans mot de passe) : connectez-y le téléphone, l'assistant s'ouvre tout seul
   (sinon, ouvrez http://10.42.0.1). Compte, puis Wi-Fi de la maison.
4. Reconnectez le téléphone à votre Wi-Fi et ouvrez **http://heim.local**.

Avec un câble réseau (Pi 3, Pi 4), pas de réseau Heim : ouvrez directement http://heim.local.
Compte système (console) : `heim-admin`, mot de passe `heim` (ou le secret `PI_PASSWORD` du dépôt) ;
SSH est coupé.

**Sur un Raspberry Pi OS Lite déjà installé** (Bookworm) :

```sh
curl -fsSL https://raw.githubusercontent.com/PROPRIETAIRE/heim/main/packaging/install.sh | sudo HEIM_REPO=PROPRIETAIRE/heim bash
```

## Publier une version

Le dépôt doit être public (les Heim installés lisent ses releases sans compte).

```sh
# Cargo.toml : version = "0.10.1"
git commit -am "0.6.1"
git tag v0.6.1
git push && git push --tags
```

GitHub Actions (`.github/workflows/release.yml`) compile Heim pour aarch64, armv7, x86_64 et
Windows, publie la release avec `SHA256SUMS`, puis fabrique l'image SD (environ 45 min).
Les Heim installés la proposent dans Réglages > Système (vérification toute seule deux fois
par jour). Le dépôt est gravé dans le binaire au moment de la compilation (`HEIM_REPO`).

## Lancer sur un PC

```sh
cargo build --release

# Démo sans matériel : lampes, prise, capteurs et sirène virtuels
HEIM_DEMO=1 ./target/release/heim
# puis http://<ip>:8090
```

Avec Zigbee2MQTT (qui tourne déjà avec son broker Mosquitto) :

```sh
HEIM_MQTT_HOST=127.0.0.1 HEIM_TOKEN=un-secret ./target/release/heim
```

## Configuration (variables d'environnement)

| Variable | Défaut | Rôle |
| --- | --- | --- |
| `HEIM_HTTP` | `0.0.0.0:8090` | Adresse d'écoute (8090 pour cohabiter avec Home Assistant sur 8123) |
| `HEIM_DATA_DIR` | `./data` | Dossier de la base `heim.db` |
| `HEIM_TOKEN` | aucun | Jeton technique (scripts) : donne les droits admin. Les personnes utilisent leur compte. |
| `HEIM_MQTT_HOST` | aucun | Active Zigbee2MQTT au premier démarrage (ensuite, tout se règle dans l'interface) |
| `HEIM_MQTT_PORT` | `1883` | |
| `HEIM_MQTT_USER` / `HEIM_MQTT_PASSWORD` | aucun | |
| `HEIM_Z2M_TOPIC` | `zigbee2mqtt` | Topic de base de Zigbee2MQTT |
| `HEIM_DEMO` | `0` | `1` pour les appareils virtuels |
| `RUST_LOG` | `heim=info,warn` | `heim=debug` pour tout voir |
| `HEIM_HTTP_ALSO` | aucun | Adresses d'écoute en plus, séparées par des virgules |
| `HEIM_NETSETUP` | `0` | `1` : Heim gère le Wi-Fi du Pi (NetworkManager), réseau « Heim-XXXX » si rien n'est relié |

## API

REST sous `/api`, JSON partout. Les erreurs ont la forme `{"error": "message lisible"}`.

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
POST   /api/scenes/{id}/activate     {"minutes": 120} (facultatif : activer pour un temps)
POST   /api/scenes/{id}/deactivate | /duplicate
POST   /api/scenes/snapshot          {"entities": ["light.salon"]}   état actuel, comme réglages de scène
POST   /api/scenes/preview           {"targets": {"light.salon": {"on": true, "color": "#ff8800"}}}   aperçu en direct
POST   /api/scenes/preview/end       tout revient comme avant
DELETE /api/scenes/{id}

GET    /api/timers
PUT    /api/timers/{id}                  {"name": "Cafetière", "duration_s": 1800, "on_finish": {"entity_id": "switch.prise_cafe", "action": "turn_off"}}
POST   /api/timers/{id}/control          {"op": "start"} | {"op": "start", "duration_s": 600} | {"op": "pause"} | {"op": "resume"} | {"op": "cancel"}
DELETE /api/timers/{id}

POST   /api/entities/{id}/turn_on_for    {"minutes": 30}
POST   /api/rooms/rename                 {"from": "Salon", "to": "Séjour"}

GET    /api/devices/{id}/controls        réglages d'un appareil (id = adresse IEEE Zigbee ou tuya:<id>)
POST   /api/devices/{id}/set             {"values": {"color": {"hex": "#ff8800"}, "transition": 2}}
POST   /api/devices/{id}/action          {"action": "ota_update"} | ota_check | interview | {"action": "remove", "force": true}
GET    /api/power_on
PUT    /api/power_on                     {"behavior": "previous", "apply_new": true}   previous | on | off
POST   /api/automations/{id}/op       {"state": "rearm"} | {"state": true} | {"enabled": false} | {"run": true}
GET    /api/logbook?entity_id=&kind=&q=&before=&limit=   journal, le plus récent d'abord
GET    /api/history/{entity_id}?hours=24&points=200      courbe d'un capteur
GET    /api/groups  |  PUT /api/groups   [{"name": "Tout le salon", "room": "Salon", "members": ["light.a", "light.b"]}]
POST   /api/entities/{id}/battery_reset  pile changée
GET    /api/backups  |  POST /api/backups  |  GET /api/backups/{nom}   sauvegardes de nuit
POST   /api/system/reboot  |  POST /api/system/poweroff
GET    /api/integrations/catalog
PUT    /api/integrations/{nom}/enabled   {"enabled": false}

GET    /api/automations
PUT    /api/automations/{id}             {"name": "...", "triggers": [...], "conditions": [...], "actions": [...]}
POST   /api/automations/{id}/run
DELETE /api/automations/{id}

GET    /api/settings/{clé}               zigbee2mqtt | home | location | alerts | ui.*
PUT    /api/settings/{clé}
GET    /api/system
GET    /api/backup                       fichier .db
POST   /api/restore                      fichier .db, chargé au prochain démarrage

GET    /api/security                     état, réglages (sans le code), historique, photos
PUT    /api/security/config              {"sensors": [{"entity_id": "binary_sensor.porte", "role": "perimeter", "entry": true}], "sirens": [...], "lights": [...], "cameras": [{"name": "Entrée", "snapshot_url": "http://..."}], "exit_delay_s": 60, "entry_delay_s": 30}
PUT    /api/security/code                {"current": "1234", "new": "5678"}   (new vide = retirer)
POST   /api/security/arm                 {"mode": "away" | "night", "force": false, "code": null}   409 si ouvert / injoignable
POST   /api/security/disarm              {"code": "1234"}
POST   /api/security/test_sirens         {"seconds": 3}
POST   /api/security/snapshot            {"camera": "Entrée"}
GET    /api/security/snapshots/{nom}     image JPEG

GET    /api/zigbee/pairing               {"open": true, "until": "...", "version": "2.1.0", "events": [...]}
POST   /api/zigbee/permit_join           {"seconds": 240}   (0 = fermer)

GET    /api/auth/status                  public : {"open": false, "users": [{"id": "alex", "name": "Alex"}]}
POST   /api/auth/login                   {"name": "alex", "password": "..."}   pose le cookie de session
POST   /api/auth/logout | /api/auth/password {"current": "...", "new": "..."}
GET    /api/auth/me
GET    /api/users | POST /api/users      {"name": "Camille", "role": "member", "password": "..."}
PUT    /api/users/{id}                   {"role": "guest"} | {"password": "..."}
DELETE /api/users/{id}

GET    /api/push/key                     clé publique VAPID
POST   /api/push/subscribe               {"target_id": "app_x1y2z3", "subscription": {...}}   (DELETE pour se désabonner)
POST   /api/push/test                    {"target_id": "app_x1y2z3"}

GET    /api/presence | PUT /api/presence/config | POST /api/presence/scan
GET    /api/remote                       Tailscale détecté ? adresse https ?

GET    /api/setup                        public : assistant nécessaire ? réseau ?
POST   /api/setup/scan | /api/setup/wifi {"ssid", "password"} | /api/setup/done
GET    /api/update | POST /api/update/check | POST /api/update/install | PUT /api/update/settings {"repo": "..."}
POST   /api/system/restart | /api/system/reboot

GET    /api/tuya | PUT /api/tuya         {"enabled": true, "devices": [{"id", "name", "key", "ip", "version"}]}
POST   /api/tuya/import                  contenu d'un devices.json de tinytuya
POST   /api/tuya/scan                    appareils qui s'annoncent sur le réseau
GET    /api/matter | PUT /api/matter     {"enabled": true, "url": "192.168.1.50:5580"}
POST   /api/matter/commission            {"code": "MT:..."}
POST   /api/matter/remove                {"node_id": 4}

POST   /api/notify                       {"title": "Maison", "message": "Linge terminé", "to": ["person:alex"], "except": ["ecran_cuisine"]}
GET    /api/notify/targets
PUT    /api/notify/targets/{id}          {"name": "ntfy", "kind": "webhook", "url": "https://ntfy.sh/mon-sujet"}
GET    /api/notify/inbox/{target}?after={id}
```

WebSocket `/api/ws` : un message `snapshot` à la connexion, puis les événements (`state_changed`,
`entity_updated`, `entity_removed`, `command_timeout`, `integration_status`, `notification`,
`security`, `zigbee_bridge`, `zigbee_device`).
Le client peut envoyer :

```json
{"type": "hello", "target_id": "tel_alex", "name": "Téléphone Alex"}
{"type": "command", "entity_id": "light.salon", "action": "toggle"}
```

## Architecture

```text
src/
  model.rs        entités, états, cibles, actions
  bus.rs          bus d'événements (tokio broadcast)
  registry.rs     acteur propriétaire de tous les états (pas de verrou partagé)
  scenes.rs       scènes avec état
  timers.rs       minuteurs à heure de fin, « allumer pour X min »
  automations.rs  automatismes Quand / Si / Alors
  accounts.rs     comptes, rôles, sessions
  push.rs         notifications push (VAPID, chiffrement RFC 8291)
  presence.rs     présence par le Wi-Fi
  update.rs       mises à jour par GitHub Releases
  netsetup.rs     Wi-Fi du Pi, réseau « Heim-XXXX » du premier démarrage
  security.rs     alarme (modes, délais, sirènes, caméras, code)
  alerts.rs       alertes intégrées (piles, appareils muets, fuites)
  sun.rs          lever / coucher du soleil calculés sur place
  settings.rs     réglages modifiables depuis l'interface
  api_more.rs     routes réglages, système, sauvegarde, automatismes
  api_security.rs routes alarme et appairage Zigbee
  api_accounts.rs routes connexion, comptes, push
  api_presence.rs routes présence et accès à distance
  notify.rs       cibles et envoi des notifications
  store.rs        SQLite (WAL, thread d'écriture dédié, écritures groupées)
  api.rs          REST + WebSocket (axum)
  integrations/
    mod.rs        supervision et relance des intégrations
    zigbee2mqtt.rs
    tuya.rs, tuya_proto.rs   Tuya local (3.3 / 3.4 / 3.5)
    matter.rs                client du serveur Matter
    virtual_dev.rs
static/index.html interface compilée (générée par `ui/`, ne pas modifier à la main)
static/sw.js, manifest.webmanifest, icônes : l'application installable (écran d'accueil, push)
ui/               sources de l'interface (Svelte 5)
tests/core.rs     tests de bout en bout (appareils virtuels)
tests/security.rs l'alarme de bout en bout
tests/accounts.rs comptes, droits et présence (serveur HTTP réel)
tests/tuya.rs     Tuya contre de faux appareils bâtis sur tinytuya (fake_tuya.py)
tests/matter.rs   Matter contre un faux serveur Matter (fake_matter.py)
packaging/        image SD (pi-gen), service systemd, garde de retour arrière, install.sh
```

## Raspberry Pi

Compiler sur un Pi Zero 2 W est très lent (et l'optimisation `lto` peut manquer de mémoire).
Mieux vaut compiler sur un PC :

```sh
pip install ziglang cargo-zigbuild
rustup target add aarch64-unknown-linux-musl          # Raspberry Pi OS 64 bits (Pi 3, Pi Zero 2 W)
rustup target add armv7-unknown-linux-musleabihf      # Raspberry Pi OS 32 bits
cargo zigbuild --release --target aarch64-unknown-linux-musl
scp target/aarch64-unknown-linux-musl/release/heim pi@raspberrypi.local:~
```

Le workflow `release.yml` produit ces binaires à chaque tag. Le service systemd est dans
`packaging/rootfs/heim.service`.

## Sécurité

Voir [SECURITY.md](SECURITY.md) : comptes, rôles, protections du navigateur, secrets, service
système, et comment signaler une faille. En bref : gardez au moins un compte, passez par
Tailscale pour l'accès depuis dehors, et n'ouvrez jamais le port de Heim sur Internet.

## Licence

Apache-2.0. Zigbee2MQTT (GPL-3.0) est utilisé comme service séparé, via MQTT : aucun de son code n'est repris.
