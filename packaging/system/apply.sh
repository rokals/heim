#!/bin/sh
# Appliqué en root sur la box, à chaque nouvelle version de Heim (voir rootfs/heim-system).
# Archive : ce script et les fichiers de packaging/rootfs, à plat. Il doit pouvoir repasser
# autant de fois qu'on veut sans rien casser.
set -eu
S="$HEIM_SYSTEM_SRC"
# La version de Zigbee2MQTT qui va avec cette version de Heim.
Z2M_VERSION=2.14.2

install -d /usr/local/lib/heim /etc/polkit-1/rules.d /etc/systemd/journald.conf.d /etc/NetworkManager/dnsmasq-shared.d
install -m 755 "$S/heim-guard" /usr/local/lib/heim/heim-guard
install -m 755 "$S/heim-system" /usr/local/lib/heim/heim-system
for unit in heim.service zigbee2mqtt.service heim-system.service heim-system.path; do
  install -m 644 "$S/$unit" "/etc/systemd/system/$unit"
done
install -m 644 "$S/50-heim.rules" /etc/polkit-1/rules.d/50-heim.rules
install -m 644 "$S/heim-captive.conf" /etc/NetworkManager/dnsmasq-shared.d/heim-captive.conf
install -m 644 "$S/heim-journald.conf" /etc/systemd/journald.conf.d/heim.conf
systemctl daemon-reload
systemctl enable heim-system.path >/dev/null 2>&1 || true
systemctl restart polkit 2>/dev/null || true

# Zigbee2MQTT : mis à jour seulement s'il est installé (image SD, ou install.sh HEIM_ZIGBEE=1).
PKG=/opt/zigbee2mqtt/node_modules/zigbee2mqtt/package.json
if [ -f "$PKG" ] && command -v npm >/dev/null 2>&1; then
  current=$(sed -n 's/^ *"version": *"\([^"]*\)".*/\1/p' "$PKG" | head -n 1)
  if [ "$current" != "$Z2M_VERSION" ]; then
    echo "Zigbee2MQTT $current -> $Z2M_VERSION"
    (cd /opt/zigbee2mqtt && npm install --omit=dev --no-audit --no-fund --no-update-notifier "zigbee2mqtt@$Z2M_VERSION")
    npm cache clean --force >/dev/null 2>&1 || true
    systemctl try-restart zigbee2mqtt.service || true
  fi
fi

# Changements propres à une version : ajoutez-les ici, protégés pour pouvoir repasser.
# (exemple : command -v wg >/dev/null || apt-get install -y wireguard-tools)
echo "ok"
