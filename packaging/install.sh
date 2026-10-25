#!/bin/bash
# Installe Heim sur un Raspberry Pi OS Lite (Bookworm, 64 ou 32 bits) déjà en marche.
# Même résultat que l'image SD : service, mises à jour, Wi-Fi « Heim-XXXX » si besoin.
#
#   curl -fsSL https://raw.githubusercontent.com/PROPRIETAIRE/heim/main/packaging/install.sh | sudo HEIM_REPO=PROPRIETAIRE/heim bash
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
for f in heim.service heim-guard 50-heim.rules heim-captive.conf; do
  curl -fsSL -o "$TMP/$f" "$RAW/$f"
done
install -m 755 "$TMP/heim-guard" /usr/local/lib/heim/heim-guard
install -m 644 "$TMP/heim.service" /etc/systemd/system/heim.service
install -m 644 "$TMP/50-heim.rules" /etc/polkit-1/rules.d/50-heim.rules
install -m 644 "$TMP/heim-captive.conf" /etc/NetworkManager/dnsmasq-shared.d/heim-captive.conf

# Sans NetworkManager actif (ancienne installation avec dhcpcd), Heim ne touche pas au Wi-Fi.
if ! systemctl is-active --quiet NetworkManager; then
  echo "  NetworkManager inactif : Heim ne gérera pas le Wi-Fi."
  sed -i 's/^Environment=HEIM_NETSETUP=1/# Environment=HEIM_NETSETUP=1/' /etc/systemd/system/heim.service
fi
# Port 80 déjà pris (autre serveur web) : Heim reste sur 8090.
if ss -ltn 'sport = :80' | grep -q LISTEN; then
  echo "  Port 80 occupé : Heim écoutera seulement sur 8090."
  sed -i 's/^Environment=HEIM_HTTP=0.0.0.0:80/Environment=HEIM_HTTP=0.0.0.0:8090/; s/^Environment=HEIM_HTTP_ALSO=.*/Environment=HEIM_HTTP_ALSO=/' /etc/systemd/system/heim.service
fi
systemctl daemon-reload
systemctl enable --now heim.service
systemctl restart polkit 2>/dev/null || true

IP=$(hostname -I | awk '{print $1}')
echo
echo "Heim est installé. Ouvrez http://$(hostname).local ou http://$IP"
