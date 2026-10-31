//! Quel firmware porte la clé Zigbee ? On lui pose la question, comme le fait ZHA.
//!
//! Le nom USB ne suffit pas (une Sonoff « Dongle Plus » peut être en Z-Stack ou reflashée en
//! EmberZNet). Avant de lancer Zigbee2MQTT, Heim ouvre le port série et essaie les deux
//! poignées de main, à 115200 puis 460800 bauds :
//! - EmberZNet (Silicon Labs, pilote `ember`) : trame ASH « RST », la clé répond « RSTACK »
//!   (octets `C1 02`) ;
//! - Z-Stack (Texas Instruments, pilote `zstack`) : `SYS_PING` puis `SYS_VERSION`, la clé
//!   répond `FE … 61 01` puis sa version et la date de son firmware.
//!
//! Une clé qui répond autre chose porte sans doute un firmware Thread ou multiprotocole, pas
//! du Zigbee seul. Zigbee2MQTT doit être arrêté pendant l'essai : un port série ne sert qu'à
//! un programme à la fois.

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Probe {
    /// `ember`, `zstack`, ou rien si on ne sait pas.
    pub adapter: Option<&'static str>,
    /// En clair, pour l'interface.
    pub detail: String,
    pub baud: Option<u32>,
}

/// Une trame ASH (EmberZNet) de réponse à la réinitialisation, après dé-bourrage ?
pub fn is_rstack(bytes: &[u8]) -> bool {
    // Les octets réservés sont échappés par 7D xx ^ 20 : on les remet avant de chercher.
    let mut plain = Vec::with_capacity(bytes.len());
    let mut esc = false;
    for &b in bytes {
        if esc {
            plain.push(b ^ 0x20);
            esc = false;
        } else if b == 0x7D {
            esc = true;
        } else {
            plain.push(b);
        }
    }
    plain.windows(2).any(|w| w == [0xC1, 0x02])
}

/// La réponse Z-Stack à `SYS_PING` (`FE 02 61 01 …`).
pub fn is_znp_ping(bytes: &[u8]) -> bool {
    bytes.windows(4).any(|w| w == [0xFE, 0x02, 0x61, 0x01])
}

/// `SYS_VERSION` : « Z-Stack 3.x.0, firmware du 2023-05-07 ».
pub fn znp_version(bytes: &[u8]) -> Option<String> {
    let i = bytes
        .windows(4)
        .position(|w| w[0] == 0xFE && w[2] == 0x61 && w[3] == 0x02)?;
    let len = bytes[i + 1] as usize;
    let data = bytes.get(i + 4..i + 4 + len)?;
    let (major, minor, maint) = (*data.get(2)?, *data.get(3)?, *data.get(4)?);
    let mut text = format!("Z-Stack {major}.{minor}.{maint}");
    if let Some(rev) = data.get(5..9) {
        let r = u32::from_le_bytes(rev.try_into().ok()?);
        let s = r.to_string();
        if s.len() == 8 {
            text.push_str(&format!(
                ", firmware du {}-{}-{}",
                &s[..4],
                &s[4..6],
                &s[6..]
            ));
        } else if r != 0 {
            text.push_str(&format!(", révision {r}"));
        }
    }
    Some(text)
}

/// Pose la question à la clé. Bloquant (environ 2 à 6 s) : à lancer dans `spawn_blocking`.
#[cfg(unix)]
pub fn probe(path: &str) -> Probe {
    let mut answered = false;
    for baud in [115_200u32, 460_800] {
        let Ok(port) = serial::Port::open(path, baud) else {
            return Probe {
                adapter: None,
                detail: "Impossible d'ouvrir la clé : est-elle utilisée par un autre programme, ou Heim n'a-t-il pas encore le droit de l'ouvrir (mise à jour système) ?".into(),
                baud: None,
            };
        };
        // EmberZNet : annuler, puis réinitialiser (RST), et attendre RSTACK.
        port.flush();
        port.write(&[0x1A, 0xC0, 0x38, 0xBC, 0x7E]);
        let r = port.read_for(1500);
        if is_rstack(&r) {
            return Probe {
                adapter: Some("ember"),
                detail: "Firmware EmberZNet (Silicon Labs) : pilote « ember ».".into(),
                baud: Some(baud),
            };
        }
        answered |= !r.is_empty();
        // Z-Stack : sortir du chargeur de démarrage, puis SYS_PING et SYS_VERSION.
        port.flush();
        port.write(&[0xEF]);
        std::thread::sleep(std::time::Duration::from_millis(1000));
        port.flush();
        port.write(&[0xFE, 0x00, 0x21, 0x01, 0x20]);
        let r = port.read_for(1500);
        if is_znp_ping(&r) {
            port.write(&[0xFE, 0x00, 0x21, 0x02, 0x23]);
            let v = port.read_for(1000);
            let detail = match znp_version(&v) {
                Some(v) => format!("{v} (Texas Instruments) : pilote « zstack »."),
                None => "Firmware Z-Stack (Texas Instruments) : pilote « zstack ».".into(),
            };
            return Probe {
                adapter: Some("zstack"),
                detail,
                baud: Some(baud),
            };
        }
        answered |= !r.is_empty();
    }
    Probe {
        adapter: None,
        detail: if answered {
            "La clé répond, mais ni en EmberZNet ni en Z-Stack : elle porte sans doute un firmware Thread ou multiprotocole. Pour Zigbee2MQTT, il faut la reflasher en Zigbee seul.".into()
        } else {
            "La clé ne répond pas (ni EmberZNet, ni Z-Stack). Débranchez-la quelques secondes et réessayez ; sinon, son firmware est peut-être abîmé.".into()
        },
        baud: None,
    }
}

#[cfg(not(unix))]
pub fn probe(_path: &str) -> Probe {
    Probe {
        adapter: None,
        detail: "Détection du firmware indisponible sur ce système.".into(),
        baud: None,
    }
}

/// Le port série en mode brut, sans contrôle de flux, avec des lectures à délai.
#[cfg(unix)]
mod serial {
    use std::ffi::CString;
    use std::time::{Duration, Instant};

    pub struct Port(libc::c_int);

    impl Port {
        pub fn open(path: &str, baud: u32) -> Result<Port, String> {
            let c = CString::new(path).map_err(|e| e.to_string())?;
            // SAFETY : chemin terminé par un zéro ; descripteur fermé dans Drop.
            let fd = unsafe {
                libc::open(
                    c.as_ptr(),
                    libc::O_RDWR | libc::O_NOCTTY | libc::O_NONBLOCK | libc::O_CLOEXEC,
                )
            };
            if fd < 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let port = Port(fd);
            let speed = match baud {
                460_800 => libc::B460800,
                _ => libc::B115200,
            };
            // SAFETY : `t` est rempli par tcgetattr avant d'être modifié.
            unsafe {
                let mut t: libc::termios = std::mem::zeroed();
                if libc::tcgetattr(fd, &mut t) != 0 {
                    return Err(std::io::Error::last_os_error().to_string());
                }
                libc::cfmakeraw(&mut t);
                t.c_cflag |= libc::CLOCAL | libc::CREAD;
                t.c_cflag &= !(libc::CRTSCTS | libc::CSTOPB | libc::PARENB);
                t.c_cc[libc::VMIN] = 0;
                t.c_cc[libc::VTIME] = 0;
                libc::cfsetispeed(&mut t, speed);
                libc::cfsetospeed(&mut t, speed);
                if libc::tcsetattr(fd, libc::TCSANOW, &t) != 0 {
                    return Err(std::io::Error::last_os_error().to_string());
                }
            }
            Ok(port)
        }

        /// Vide ce qui attend dans les deux sens.
        pub fn flush(&self) {
            // SAFETY : descripteur ouvert.
            unsafe {
                libc::tcflush(self.0, libc::TCIOFLUSH);
            }
        }

        pub fn write(&self, data: &[u8]) {
            // SAFETY : tampon valide de la bonne longueur.
            unsafe {
                libc::write(self.0, data.as_ptr().cast(), data.len());
                libc::tcdrain(self.0);
            }
        }

        /// Tout ce qui arrive pendant `ms` millisecondes.
        pub fn read_for(&self, ms: u64) -> Vec<u8> {
            let end = Instant::now() + Duration::from_millis(ms);
            let mut out = Vec::new();
            let mut buf = [0u8; 256];
            while Instant::now() < end {
                let mut pfd = libc::pollfd {
                    fd: self.0,
                    events: libc::POLLIN,
                    revents: 0,
                };
                let left = end.saturating_duration_since(Instant::now()).as_millis() as i32;
                // SAFETY : un seul pollfd valide.
                let n = unsafe { libc::poll(&mut pfd, 1, left.max(1)) };
                if n <= 0 {
                    continue;
                }
                // SAFETY : tampon valide de la bonne longueur.
                let r = unsafe { libc::read(self.0, buf.as_mut_ptr().cast(), buf.len()) };
                if r > 0 {
                    out.extend_from_slice(&buf[..r as usize]);
                    if out.len() > 4096 {
                        break;
                    }
                }
            }
            out
        }
    }

    impl Drop for Port {
        fn drop(&mut self) {
            // SAFETY : descripteur ouvert par open().
            unsafe {
                libc::close(self.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_answers() {
        // RSTACK : 1A C1 02 0B 0A 52 7E (avec des octets parasites devant).
        assert!(is_rstack(&[0x00, 0x1A, 0xC1, 0x02, 0x0B, 0x0A, 0x52, 0x7E]));
        assert!(!is_rstack(&[0xFE, 0x02, 0x61, 0x01]));
        assert!(is_znp_ping(&[
            0x00, 0xFE, 0x02, 0x61, 0x01, 0x59, 0x06, 0x3D
        ]));
        // SYS_VERSION : transport 2, produit 1, 2.7.1, révision 20230507.
        let rev = 20230507u32.to_le_bytes();
        let mut v = vec![0xFE, 0x0A, 0x61, 0x02, 0x02, 0x01, 0x02, 0x07, 0x01];
        v.extend_from_slice(&rev);
        v.push(0x00);
        assert_eq!(
            znp_version(&v).as_deref(),
            Some("Z-Stack 2.7.1, firmware du 2023-05-07")
        );
    }
}
