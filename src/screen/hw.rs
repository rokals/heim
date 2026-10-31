//! Le matériel : le framebuffer de l'écran SPI (`/dev/fbN`, pilote fbtft) et son tactile
//! résistif (ADS7846, `/dev/input/eventN`). Rien que Linux, rien que des fichiers.

use std::path::{Path, PathBuf};

/// Le framebuffer de l'écran : on cherche un pilote fbtft (ili9486, ili9341…), pas la sortie
/// HDMI.
pub fn find_fb() -> Option<(PathBuf, String)> {
    let dir = std::fs::read_dir("/sys/class/graphics").ok()?;
    let mut list: Vec<_> = dir.flatten().map(|e| e.path()).collect();
    list.sort();
    for p in list {
        let name = p.file_name()?.to_string_lossy().to_string();
        if !name.starts_with("fb") {
            continue;
        }
        let driver = std::fs::read_to_string(p.join("name")).unwrap_or_default();
        let d = driver.trim().to_ascii_lowercase();
        if d.contains("ili9") || d.contains("fb_") || d.contains("hx8357") || d.contains("st7") {
            return Some((
                PathBuf::from(format!("/dev/{name}")),
                driver.trim().to_string(),
            ));
        }
    }
    None
}

/// Le tactile de l'écran.
pub fn find_touch() -> Option<PathBuf> {
    let dir = std::fs::read_dir("/sys/class/input").ok()?;
    for e in dir.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.starts_with("event") {
            continue;
        }
        let dev = std::fs::read_to_string(e.path().join("device/name")).unwrap_or_default();
        let d = dev.to_ascii_lowercase();
        if d.contains("ads7846") || d.contains("touchscreen") || d.contains("xpt2046") {
            return Some(PathBuf::from(format!("/dev/input/{name}")));
        }
    }
    None
}

pub struct Fb {
    file: std::fs::File,
    pub w: i32,
    pub h: i32,
    bpp: u32,
    stride: usize,
}

impl Fb {
    pub fn open(dev: &Path) -> Result<Fb, String> {
        let name = dev
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let sys = PathBuf::from("/sys/class/graphics").join(&name);
        let read = |f: &str| std::fs::read_to_string(sys.join(f)).unwrap_or_default();
        let size = read("virtual_size");
        let (w, h) = size
            .trim()
            .split_once(',')
            .and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)))
            .ok_or("taille de l'écran inconnue")?;
        let bpp: u32 = read("bits_per_pixel").trim().parse().unwrap_or(16);
        let stride: usize = read("stride")
            .trim()
            .parse()
            .unwrap_or(w as usize * bpp as usize / 8);
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(dev)
            .map_err(|e| format!("{} : {e}", dev.display()))?;
        Ok(Fb {
            file,
            w,
            h,
            bpp,
            stride,
        })
    }

    /// Envoie l'image (RGB565, taille logique `w` × h) à l'écran, tournée selon `rot`.
    pub fn show(&self, px: &[u16], w: i32, rot: super::Rot) -> std::io::Result<()> {
        use std::os::unix::fs::FileExt;
        let (lw, lh) = rot.size();
        let mut buf = Vec::with_capacity(self.stride * self.h as usize);
        for ny in 0..self.h {
            for nx in 0..self.w {
                let (lx, ly) = rot.to_logical(nx, ny);
                let p = if lx >= 0 && ly >= 0 && lx < lw.min(w) && ly < lh {
                    px[(ly * w + lx) as usize]
                } else {
                    0
                };
                if self.bpp == 32 {
                    let r = ((p >> 11) & 0x1F) as u8;
                    let g = ((p >> 5) & 0x3F) as u8;
                    let b = (p & 0x1F) as u8;
                    buf.extend_from_slice(&[
                        b << 3 | b >> 2,
                        g << 2 | g >> 4,
                        r << 3 | r >> 2,
                        0xFF,
                    ]);
                } else {
                    buf.extend_from_slice(&p.to_le_bytes());
                }
            }
            buf.resize((ny as usize + 1) * self.stride, 0);
        }
        self.file.write_all_at(&buf, 0)
    }
}

/// Un appui (coordonnées brutes du tactile, 0-4095) : `down` à l'appui, `up` au relâché.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Touch {
    Down(i32, i32),
    Up(i32, i32),
}

/// Lit le tactile dans un fil à part et envoie appuis et relâchés.
pub fn spawn_touch(dev: PathBuf, tx: tokio::sync::mpsc::Sender<Touch>) {
    std::thread::spawn(move || {
        use std::io::Read;
        let Ok(mut f) = std::fs::File::open(&dev) else {
            tracing::warn!("écran : tactile {} illisible", dev.display());
            return;
        };
        let size = std::mem::size_of::<libc::input_event>();
        let mut buf = vec![0u8; size];
        let (mut x, mut y, mut down, mut was_down) = (0i32, 0i32, false, false);
        loop {
            if f.read_exact(&mut buf).is_err() {
                return;
            }
            // SAFETY : tampon de la taille exacte d'un input_event, lu d'un seul coup.
            let ev: libc::input_event = unsafe { std::ptr::read_unaligned(buf.as_ptr().cast()) };
            match (ev.type_, ev.code) {
                (3, 0) => x = ev.value,             // EV_ABS, ABS_X
                (3, 1) => y = ev.value,             // EV_ABS, ABS_Y
                (1, 0x14a) => down = ev.value != 0, // EV_KEY, BTN_TOUCH
                (0, 0) => {
                    // EV_SYN : un état complet.
                    let msg = match (was_down, down) {
                        (false, true) => Some(Touch::Down(x, y)),
                        (true, false) => Some(Touch::Up(x, y)),
                        _ => None,
                    };
                    was_down = down;
                    if let Some(m) = msg
                        && tx.blocking_send(m).is_err()
                    {
                        return;
                    }
                }
                _ => {}
            }
        }
    });
}
