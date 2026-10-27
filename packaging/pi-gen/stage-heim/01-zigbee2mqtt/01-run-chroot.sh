#!/bin/bash -e
# Étape pi-gen : Zigbee2MQTT sur la box, pour que Heim se suffise à lui-même.
# Node.js 22 (NodeSource : Debian Bookworm n'a que Node 18) et Zigbee2MQTT depuis npm.
# Le service reste éteint tant que Heim n'a pas écrit la configuration (clé USB choisie).
#
# Zigbee2MQTT (GPL-3.0) est installé tel quel, comme programme à part : aucun de son code
# n'est repris dans Heim, qui lui parle en MQTT.

Z2M_VERSION="${Z2M_VERSION:-2.14.2}"

curl -fsSL https://deb.nodesource.com/setup_22.x | bash -
apt-get install -y --no-install-recommends nodejs

install -d /opt/zigbee2mqtt
cd /opt/zigbee2mqtt
npm install --omit=dev --no-audit --no-fund --no-update-notifier "zigbee2mqtt@${Z2M_VERSION}"
npm cache clean --force

id -u heim >/dev/null 2>&1 || useradd --system --home-dir /var/lib/heim --shell /usr/sbin/nologin heim
usermod -aG dialout heim
install -d -m 700 -o heim -g heim /var/lib/heim/zigbee2mqtt
systemctl enable zigbee2mqtt.service
