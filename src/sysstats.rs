//! Les performances de la box : mémoire, processeur, carte SD, température, et ce que prend
//! chaque morceau (Heim, Zigbee2MQTT…).
//!
//! Tout est lu directement dans Linux (`/proc`, `/sys`), sans outil ni dépendance : un relevé
//! toutes les 5 secondes coûte quelques microsecondes. Les mesures deviennent aussi des
//! entités (appareil « Box Heim ») mises à jour chaque minute, pour les tuiles, les courbes
//! et les automatismes. Ailleurs que sous Linux, seules les valeurs disponibles sont données.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::json;

use crate::model::{Attributes, Category, Domain, EntityMeta};
use crate::registry::RegistryHandle;

pub const INTEGRATION: &str = "system";
pub const DEVICE_ID: &str = "system:box";

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Usage {
    pub total: u64,
    pub used: u64,
    pub percent: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Process {
    pub name: String,
    /// Mémoire réellement occupée (RSS), en octets.
    pub memory: u64,
    pub cpu: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Snapshot {
    pub memory: Option<Usage>,
    pub swap: Option<Usage>,
    pub cpu: Option<f64>,
    pub cores: usize,
    pub load: Option<[f64; 3]>,
    pub disk: Option<Usage>,
    pub temperature: Option<f64>,
    pub uptime_s: Option<u64>,
    pub processes: Vec<Process>,
    pub at: Option<chrono::DateTime<chrono::Utc>>,
}

/// `MemTotal: 948304 kB` → octets.
pub fn parse_meminfo(text: &str) -> (Option<Usage>, Option<Usage>) {
    let get = |k: &str| {
        text.lines().find_map(|l| {
            let rest = l.strip_prefix(k)?.strip_prefix(':')?;
            rest.split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()
                .map(|kb| kb * 1024)
        })
    };
    let mem = match (get("MemTotal"), get("MemAvailable")) {
        (Some(t), Some(a)) if t > 0 => Some(usage(t, t.saturating_sub(a))),
        _ => None,
    };
    let swap = match (get("SwapTotal"), get("SwapFree")) {
        (Some(t), Some(f)) if t > 0 => Some(usage(t, t.saturating_sub(f))),
        _ => None,
    };
    (mem, swap)
}

fn usage(total: u64, used: u64) -> Usage {
    Usage {
        total,
        used,
        percent: round1(used as f64 * 100.0 / total.max(1) as f64),
    }
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// Première ligne de `/proc/stat` : (temps actif, temps total), en ticks.
pub fn parse_cpu(text: &str) -> Option<(u64, u64)> {
    let line = text.lines().find(|l| l.starts_with("cpu "))?;
    let v: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|x| x.parse().ok())
        .collect();
    if v.len() < 4 {
        return None;
    }
    let total: u64 = v.iter().take(8).sum();
    let idle = v[3] + v.get(4).copied().unwrap_or(0);
    Some((total - idle, total))
}

/// Ticks de processeur d'un processus (`/proc/<pid>/stat`, utime + stime).
fn proc_ticks(stat: &str) -> Option<u64> {
    // Le nom est entre parenthèses et peut contenir des espaces : on repart après « ) ».
    let rest = &stat[stat.rfind(')')? + 2..];
    let f: Vec<&str> = rest.split_whitespace().collect();
    Some(f.get(11)?.parse::<u64>().ok()? + f.get(12)?.parse::<u64>().ok()?)
}

fn rss_bytes(status: &str) -> Option<u64> {
    status.lines().find_map(|l| {
        l.strip_prefix("VmRSS:")?
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .ok()
            .map(|kb| kb * 1024)
    })
}

/// À qui appartient un processus, d'après sa ligne de commande.
pub fn classify(own_pid: u32, pid: u32, cmdline: &str) -> Option<&'static str> {
    if pid == own_pid {
        return Some("Heim");
    }
    if cmdline.contains("zigbee2mqtt") {
        return Some("Zigbee2MQTT");
    }
    if cmdline.contains("matter_server") || cmdline.contains("matter-server") {
        return Some("Serveur Matter");
    }
    None
}

#[cfg(unix)]
fn disk(path: &Path) -> Option<Usage> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY : chemin C valide, structure allouée ici, lue seulement si l'appel réussit.
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    let frsize = st.f_frsize as u64;
    let total = st.f_blocks as u64 * frsize;
    let avail = st.f_bavail as u64 * frsize;
    let free = st.f_bfree as u64 * frsize;
    if total == 0 {
        return None;
    }
    // Comme « df » : l'espace réservé à root compte comme pris.
    let used = total.saturating_sub(free);
    Some(Usage {
        total: used + avail,
        used,
        percent: round1(used as f64 * 100.0 / (used + avail).max(1) as f64),
    })
}

#[cfg(not(unix))]
fn disk(_path: &Path) -> Option<Usage> {
    None
}

fn read(p: impl AsRef<Path>) -> Option<String> {
    std::fs::read_to_string(p).ok()
}

/// La température du processeur (°C), première zone thermique qui répond.
fn temperature() -> Option<f64> {
    for i in 0..4 {
        if let Some(t) = read(format!("/sys/class/thermal/thermal_zone{i}/temp"))
            .and_then(|t| t.trim().parse::<f64>().ok())
            && t > 0.0
        {
            return Some(round1(t / 1000.0));
        }
    }
    None
}

#[derive(Default)]
struct Previous {
    cpu: Option<(u64, u64)>,
    procs: std::collections::HashMap<u32, u64>,
}

pub struct Sampler {
    data_dir: PathBuf,
    last: Mutex<Snapshot>,
    prev: Mutex<Previous>,
}

impl Sampler {
    pub fn new(data_dir: PathBuf) -> Arc<Self> {
        Arc::new(Sampler {
            data_dir,
            last: Mutex::new(Snapshot::default()),
            prev: Mutex::new(Previous::default()),
        })
    }

    pub fn last(&self) -> Snapshot {
        self.last.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Un relevé complet. Le processeur se calcule entre deux relevés.
    pub fn sample(&self) -> Snapshot {
        let mut s = Snapshot {
            cores: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1),
            at: Some(chrono::Utc::now()),
            ..Default::default()
        };
        if let Some(m) = read("/proc/meminfo") {
            (s.memory, s.swap) = parse_meminfo(&m);
        }
        let cpu_now = read("/proc/stat").as_deref().and_then(parse_cpu);
        let mut prev = self.prev.lock().unwrap_or_else(|e| e.into_inner());
        let cpu_delta = match (prev.cpu, cpu_now) {
            (Some((a0, t0)), Some((a1, t1))) if t1 > t0 => {
                s.cpu = Some(round1((a1 - a0) as f64 * 100.0 / (t1 - t0) as f64));
                Some(t1 - t0)
            }
            _ => None,
        };
        prev.cpu = cpu_now.or(prev.cpu);
        s.load = read("/proc/loadavg").and_then(|l| {
            let v: Vec<f64> = l
                .split_whitespace()
                .take(3)
                .filter_map(|x| x.parse().ok())
                .collect();
            (v.len() == 3).then(|| [v[0], v[1], v[2]])
        });
        s.uptime_s = read("/proc/uptime")
            .and_then(|u| u.split_whitespace().next()?.parse::<f64>().ok())
            .map(|u| u as u64);
        s.disk = disk(&self.data_dir);
        s.temperature = temperature();

        // Ce que prend chaque morceau.
        let own = std::process::id();
        let mut by_name: std::collections::BTreeMap<&'static str, Process> = Default::default();
        let mut ticks = std::collections::HashMap::new();
        if let Ok(entries) = std::fs::read_dir("/proc") {
            for e in entries.flatten() {
                let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else {
                    continue;
                };
                let cmd = read(e.path().join("cmdline"))
                    .unwrap_or_default()
                    .replace('\0', " ");
                let Some(name) = classify(own, pid, &cmd) else {
                    continue;
                };
                let mem = read(e.path().join("status"))
                    .as_deref()
                    .and_then(rss_bytes)
                    .unwrap_or(0);
                let t = read(e.path().join("stat")).as_deref().and_then(proc_ticks);
                let p = by_name.entry(name).or_insert_with(|| Process {
                    name: name.into(),
                    ..Default::default()
                });
                p.memory += mem;
                if let (Some(t), Some(dt)) = (t, cpu_delta) {
                    ticks.insert(pid, t);
                    if let Some(t0) = prev.procs.get(&pid)
                        && t >= *t0
                    {
                        // Ticks de ce processus / ticks de tous les cœurs : la part de la machine.
                        let part = (t - t0) as f64 * 100.0 / dt as f64;
                        p.cpu = Some(round1(p.cpu.unwrap_or(0.0) + part));
                    }
                } else if let Some(t) = t {
                    ticks.insert(pid, t);
                }
            }
        }
        prev.procs = ticks;
        drop(prev);
        if by_name.is_empty() {
            // Hors Linux : au moins Heim.
            by_name.insert(
                "Heim",
                Process {
                    name: "Heim".into(),
                    memory: crate::api_more::rss_kib().unwrap_or(0) * 1024,
                    cpu: None,
                },
            );
        }
        s.processes = by_name.into_values().collect();
        *self.last.lock().unwrap_or_else(|e| e.into_inner()) = s.clone();
        s
    }
}

struct Def {
    id: &'static str,
    name: &'static str,
    kind: &'static str,
    unit: &'static str,
}

const SENSORS: &[Def] = &[
    Def {
        id: "sensor.heim_memory",
        name: "Mémoire",
        kind: "memory",
        unit: "%",
    },
    Def {
        id: "sensor.heim_cpu",
        name: "Processeur",
        kind: "cpu",
        unit: "%",
    },
    Def {
        id: "sensor.heim_disk",
        name: "Carte SD",
        kind: "disk",
        unit: "%",
    },
    Def {
        id: "sensor.heim_temperature",
        name: "Température du processeur",
        kind: "temperature",
        unit: "°C",
    },
];

fn value_of(s: &Snapshot, kind: &str) -> Option<f64> {
    match kind {
        "memory" => s.memory.as_ref().map(|u| u.percent.round()),
        "cpu" => s.cpu.map(f64::round),
        "disk" => s.disk.as_ref().map(|u| u.percent.round()),
        "temperature" => s.temperature.map(f64::round),
        _ => None,
    }
}

/// Relevés toutes les 5 s (pour la page Performances), entités mises à jour chaque minute.
pub fn spawn(sampler: Arc<Sampler>, registry: RegistryHandle) {
    tokio::spawn(async move {
        let first = sampler.sample();
        for d in SENSORS {
            // Le processeur se calcule au deuxième relevé ; ailleurs que sous Linux, rien.
            if value_of(&first, d.kind).is_none() && !(d.kind == "cpu" && cfg!(target_os = "linux"))
            {
                continue;
            }
            let lang = crate::i18n::house_lang();
            let mut caps = Attributes::new();
            caps.insert(
                "device_name".into(),
                json!(crate::i18n::tr(&lang, "Box Heim")),
            );
            caps.insert("kind".into(), json!(d.kind));
            caps.insert("unit".into(), json!(d.unit));
            caps.insert("model".into(), json!("Heim"));
            if d.unit == "%" {
                caps.insert("min".into(), json!(0));
                caps.insert("max".into(), json!(100));
            }
            registry
                .upsert(EntityMeta {
                    entity_id: d.id.into(),
                    domain: Domain::Sensor,
                    name: crate::i18n::tr(&lang, d.name).into_owned(),
                    integration: INTEGRATION.into(),
                    device_id: Some(DEVICE_ID.into()),
                    room: None,
                    plan: None,
                    capabilities: caps,
                    user_edited: false,
                    category: Category::Primary,
                    hidden: false,
                })
                .await;
        }
        let mut tick = tokio::time::interval(Duration::from_secs(5));
        let mut n: u64 = 0;
        loop {
            tick.tick().await;
            let s = tokio::task::spawn_blocking({
                let sampler = sampler.clone();
                move || sampler.sample()
            })
            .await
            .unwrap_or_default();
            // Le premier tour (processeur encore inconnu), puis chaque minute.
            if n % 12 == 1 {
                for d in SENSORS {
                    if let Some(v) = value_of(&s, d.kind) {
                        registry
                            .report(d.id, json!(v), Attributes::new(), true)
                            .await;
                    }
                }
            }
            n += 1;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meminfo() {
        let text = "MemTotal:         948304 kB\nMemFree:           33016 kB\nMemAvailable:     474152 kB\nSwapTotal:        102396 kB\nSwapFree:         102396 kB\n";
        let (m, s) = parse_meminfo(text);
        let m = m.unwrap();
        assert_eq!(m.total, 948304 * 1024);
        assert_eq!(m.percent, 50.0);
        assert_eq!(s.unwrap().percent, 0.0);
    }

    #[test]
    fn cpu_line() {
        let (active, total) = parse_cpu("cpu  100 0 50 800 50 0 0 0 0 0\ncpu0 1 2 3 4\n").unwrap();
        assert_eq!(total, 1000);
        assert_eq!(active, 150);
    }

    #[test]
    fn process_stat() {
        let stat = "1234 (node index.js) S 1 1234 1234 0 -1 4194560 100 0 0 0 250 50 0 0 20 0 11 0";
        assert_eq!(proc_ticks(stat), Some(300));
        assert_eq!(
            rss_bytes("Name:\theim\nVmRSS:\t   10240 kB\n"),
            Some(10240 * 1024)
        );
        assert_eq!(classify(7, 7, ""), Some("Heim"));
        assert_eq!(
            classify(
                7,
                9,
                "node /opt/zigbee2mqtt/node_modules/zigbee2mqtt/index.js"
            ),
            Some("Zigbee2MQTT")
        );
        assert_eq!(classify(7, 9, "/usr/sbin/sshd"), None);
    }

    #[test]
    fn real_sample() {
        let s = Sampler::new(std::env::temp_dir());
        s.sample();
        std::thread::sleep(Duration::from_millis(50));
        let snap = s.sample();
        if cfg!(target_os = "linux") {
            assert!(snap.memory.is_some());
            assert!(snap.disk.is_some());
            assert!(
                snap.processes
                    .iter()
                    .any(|p| p.name == "Heim" && p.memory > 0)
            );
        }
    }
}
