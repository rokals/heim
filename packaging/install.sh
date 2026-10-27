#!/bin/bash
# Installe Heim sur un Raspberry Pi OS Lite (Bookworm, 64 ou 32 bits) déjà en marche.
# Même résultat que l'image SD : service, mises à jour, Wi-Fi « Heim-XXXX » si besoin.
#
#   curl -fsSL https://raw.githubusercontent.com/PROPRIETAIRE/heim/main/packaging/install.sh | sudo HEIM_REPO=PROPRIETAIRE/heim bash
#
# Avec Zigbee2MQTT sur la box (comme l'image SD), ajoutez HEIM_ZIGBEE=1 :
#   ... | sudo HEIM_REPO=PROPRIETAIRE/heim HEIM_ZIGBEE=1 bash
set -euo pipefail

REPO="${HEIM_REPO:-}"
if [ -z "$REPO" ]; then
  echo "Indiquez le dépôt : sudo HEIM_REPO=proprietaire/heim bash install.sh" >&2
  exit 1
fi
if [ "$(id -u)" -ne 0 ]; then
  echo "À lancer avec sudo." >&2
  exit 1
fi

case "$(uname -m)" in
  aarch64) ASSET=heim-aarch64-linux ;;
  armv7l|armv6l) ASSET=heim-armv7-linux ;;
  x86_64) ASSET=heim-x86_64-linux ;;
  *) echo "Processeur $(uname -m) non pris en charge." >&2; exit 1 ;;
esac

RAW="https://raw.githubusercontent.com/$REPO/main/packaging/rootfs"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

echo "• Paquets système"
apt-get update -qq
DEBIAN_FRONTEND=noninteractive apt-get install -y -qq network-manager dnsmasq-base polkitd avahi-daemon iputils-ping iproute2 curl >/dev/null

echo "• Dernière version de Heim ($REPO)"
API="https://api.github.com/repos/$REPO/releases/latest"
URLS=$(curl -fsSL "$API" | grep -o '"browser_download_url": *"[^"]*"' | sed 's/.*"\(https[^"]*\)"/\1/')
BIN_URL=$(echo "$URLS" | grep "/$ASSET\$" || true)
SUMS_URL=$(echo "$URLS" | grep "/SHA256SUMS\$" || true)
if [ -z "$BIN_URL" ] || [ -z "$SUMS_URL" ]; then
  echo "Aucune release avec $ASSET et SHA256SUMS sur $REPO." >&2
  exit 1
fi
curl -fsSL -o "$TMP/$ASSET" "$BIN_URL"
curl -fsSL -o "$TMP/SHA256SUMS" "$SUMS_URL"
(cd "$TMP" && grep " \*\?$ASSET\$" SHA256SUMS | sha256sum -c - >/dev/null) || { echo "Empreinte incorrecte : téléchargement abîmé." >&2; exit 1; }

echo "• Installation"
id -u heim >/dev/null 2>&1 || useradd --system --home-dir /var/lib/heim --shell /usr/sbin/nologin heim
install -d -o heim -g heim /opt/heim
install -d -o heim -g heim -m 700 /var/lib/heim
install -d /usr/local/lib/heim /etc/polkit-1/rules.d /etc/NetworkManager/dnsmasq-shared.d
systemctl stop heim.service 2>/dev/null || true
install -m 755 -o heim -g heim "$TMP/$ASSET" /opt/heim/heim
for f in heim.service heim-guard 50-heim.rules heim-captive.conf zigbee2mqtt.service heim-system heim-system.service heim-system.path heim-journald.conf; do
  curl -fsSL -o "$TMP/$f" "$RAW/$f"
done
install -m 755 "$TMP/heim-guard" /usr/local/lib/heim/heim-guard
install -m 755 "$TMP/heim-system" /usr/local/lib/heim/heim-system
install -m 644 "$TMP/heim-system.service" /etc/systemd/system/heim-system.service
install -m 644 "$TMP/heim-system.path" /etc/systemd/system/heim-system.path
install -d /etc/systemd/journald.conf.d /etc/heim
install -m 644 "$TMP/heim-journald.conf" /etc/systemd/journald.conf.d/heim.conf
# Le dépôt d'où heim-system tirera les fichiers système des versions suivantes.
echo "$REPO" > /etc/heim/repo
install -m 644 "$TMP/heim.service" /etc/systemd/system/heim.service
install -m 644 "$TMP/50-heim.rules" /etc/polkit-1/rules.d/50-heim.rules
install -m 644 "$TMP/heim-captive.conf" /etc/NetworkManager/dnsmasq-shared.d/heim-captive.conf

# Les réglages propres à cette machine vont dans un complément (heim.service.d) : les mises à
# jour des fichiers système peuvent remplacer heim.service sans les perdre.
install -d /etc/systemd/system/heim.service.d
LOCAL=/etc/systemd/system/heim.service.d/local.conf
echo "[Service]" > "$LOCAL"
# Sans NetworkManager actif (ancienne installation avec dhcpcd), Heim ne touche pas au Wi-Fi.
if ! systemctl is-active --quiet NetworkManager; then
  echo "  NetworkManager inactif : Heim ne gérera pas le Wi-Fi."
  echo "Environment=HEIM_NETSETUP=0" >> "$LOCAL"
fi
# Port 80 déjà pris (autre serveur web) : Heim reste sur 8090.
if ss -ltn 'sport = :80' | grep -q LISTEN; then
  echo "  Port 80 occupé : Heim écoutera seulement sur 8090."
  printf 'Environment=HEIM_HTTP=0.0.0.0:8090\nEnvironment=HEIM_HTTP_ALSO=\n' >> "$LOCAL"
fi
if [ "${HEIM_ZIGBEE:-0}" = "1" ]; then
  echo "• Zigbee2MQTT (Node.js 22)"
  if ! node --version 2>/dev/null | grep -q '^v2[2-9]'; then
    curl -fsSL https://deb.nodesource.com/setup_22.x | bash - >/dev/null
    DEBIAN_FRONTEND=noninteractive apt-get install -y -qq nodejs >/dev/null
  fi
  install -d /opt/zigbee2mqtt
  (cd /opt/zigbee2mqtt && npm install --omit=dev --no-audit --no-fund --no-update-notifier "zigbee2mqtt@${Z2M_VERSION:-2.14.2}" >/dev/null)
  usermod -aG dialout heim
  install -d -m 700 -o heim -g heim /var/lib/heim/zigbee2mqtt
  install -m 644 "$TMP/zigbee2mqtt.service" /etc/systemd/system/zigbee2mqtt.service
  systemctl enable zigbee2mqtt.service >/dev/null 2>&1
  echo "  Zigbee2MQTT est installé : branchez la clé USB, puis Réglages › Connexions › Zigbee."
fi
systemctl daemon-reload
systemctl enable heim-system.path >/dev/null 2>&1
systemctl enable --now heim.service
systemctl restart polkit 2>/dev/null || true

IP=$(hostname -I | awk '{print $1}')
echo
echo "Heim est installé. Ouvrez http://$(hostname).local ou http://$IP"
