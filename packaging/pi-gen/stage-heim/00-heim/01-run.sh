#!/bin/bash -e
# Étape pi-gen : installe Heim dans l'image. Les fichiers sont copiés dans files/ par le
# workflow GitHub (le programme compilé et packaging/rootfs/*).

install -d "${ROOTFS_DIR}/opt/heim" "${ROOTFS_DIR}/usr/local/lib/heim"
install -d -m 700 "${ROOTFS_DIR}/var/lib/heim"
install -m 755 files/heim "${ROOTFS_DIR}/opt/heim/heim"
install -m 755 files/heim-guard "${ROOTFS_DIR}/usr/local/lib/heim/heim-guard"
install -m 644 files/heim.service "${ROOTFS_DIR}/etc/systemd/system/heim.service"
install -d "${ROOTFS_DIR}/etc/polkit-1/rules.d"
install -m 644 files/50-heim.rules "${ROOTFS_DIR}/etc/polkit-1/rules.d/50-heim.rules"
install -d "${ROOTFS_DIR}/etc/NetworkManager/dnsmasq-shared.d"
install -m 644 files/heim-captive.conf "${ROOTFS_DIR}/etc/NetworkManager/dnsmasq-shared.d/heim-captive.conf"

install -d "${ROOTFS_DIR}/etc/systemd/journald.conf.d"
install -m 644 files/heim-journald.conf "${ROOTFS_DIR}/etc/systemd/journald.conf.d/heim.conf"

on_chroot << CHROOT
id -u heim >/dev/null 2>&1 || useradd --system --home-dir /var/lib/heim --shell /usr/sbin/nologin heim
chown -R heim:heim /opt/heim /var/lib/heim
systemctl enable heim.service
systemctl enable NetworkManager.service
systemctl enable avahi-daemon.service
CHROOT
