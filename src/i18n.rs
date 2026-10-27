//! Langues des messages du Raspberry Pi (erreurs, journal, notifications).
//!
//! Le code écrit ses messages en français ; ce catalogue les traduit au dernier moment, selon
//! la langue de celui qui les lit (son compte, son navigateur) ou celle de la maison (pour les
//! notifications push). Un message absent du catalogue reste en français : rien ne casse.
//!
//! Un motif peut contenir `{}` : la partie variable (un nom, un nombre…). Dans la traduction,
//! `{}` reprend les parties dans l'ordre, ou `{0}`, `{1}`… pour changer l'ordre. Les parties
//! sont elles-mêmes traduites si elles sont au catalogue (« Absent » → « Away »).

use std::borrow::Cow;
use std::sync::RwLock;

pub const LANGS: [&str; 2] = ["fr", "en"];

static HOUSE_LANG: RwLock<String> = RwLock::new(String::new());

/// La langue de la maison (celle des notifications push). « fr » tant qu'aucune n'est choisie.
pub fn house_lang() -> String {
    let l = HOUSE_LANG.read().map(|l| l.clone()).unwrap_or_default();
    if l.is_empty() { "fr".into() } else { l }
}

pub fn set_house_lang(lang: Option<&str>) {
    if let Ok(mut l) = HOUSE_LANG.write() {
        *l = known(lang).unwrap_or("fr").to_string();
    }
}

/// Une langue connue, ou rien.
pub fn known(lang: Option<&str>) -> Option<&'static str> {
    let l = lang?.trim().get(..2)?.to_ascii_lowercase();
    LANGS.iter().copied().find(|k| *k == l)
}

/// Traduit un message français dans la langue demandée.
pub fn tr<'a>(lang: &str, text: &'a str) -> Cow<'a, str> {
    if lang == "fr" || text.is_empty() {
        return Cow::Borrowed(text);
    }
    let catalog: &[(&str, &str)] = match lang {
        "en" => EN,
        _ => return Cow::Borrowed(text),
    };
    match translate(catalog, text, 0) {
        Some(s) => Cow::Owned(s),
        None => Cow::Borrowed(text),
    }
}

fn translate(catalog: &[(&str, &str)], text: &str, depth: u8) -> Option<String> {
    // Les motifs exacts d'abord (rapides), puis ceux à trous.
    if let Some((_, en)) = catalog
        .iter()
        .find(|(fr, _)| !fr.contains("{}") && *fr == text)
    {
        return Some((*en).to_string());
    }
    // Les motifs à trous du plus précis au plus vague (« réseau « {} » introuvable » passe
    // avant « {} introuvable »).
    let mut holed: Vec<&(&str, &str)> =
        catalog.iter().filter(|(fr, _)| fr.contains("{}")).collect();
    holed.sort_by_key(|(fr, _)| std::cmp::Reverse(fr.len() - 2 * fr.matches("{}").count()));
    for (fr, en) in holed {
        if let Some(parts) = matches(fr, text) {
            let parts: Vec<String> = parts
                .into_iter()
                .map(|p| {
                    if depth < 2 {
                        translate(catalog, p, depth + 1).unwrap_or_else(|| p.to_string())
                    } else {
                        p.to_string()
                    }
                })
                .collect();
            return Some(fill(en, &parts));
        }
    }
    None
}

/// « Bonjour {} et {} » contre « Bonjour Alex et Léa » → ["Alex", "Léa"].
fn matches<'t>(pattern: &str, text: &'t str) -> Option<Vec<&'t str>> {
    let pieces: Vec<&str> = pattern.split("{}").collect();
    let first = pieces[0];
    let mut rest = text.strip_prefix(first)?;
    let mut out = Vec::new();
    for (i, lit) in pieces.iter().enumerate().skip(1) {
        let last = i == pieces.len() - 1;
        if last {
            if lit.is_empty() {
                out.push(rest);
                rest = "";
            } else {
                let cut = rest.strip_suffix(lit)?;
                out.push(cut);
                rest = "";
            }
        } else {
            if lit.is_empty() {
                return None; // deux trous collés : trop ambigu
            }
            let at = rest.find(lit)?;
            out.push(&rest[..at]);
            rest = &rest[at + lit.len()..];
        }
    }
    (rest.is_empty() && out.iter().all(|p| !p.is_empty())).then_some(out)
}

fn fill(template: &str, parts: &[String]) -> String {
    let mut out = String::new();
    let mut next = 0;
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        if let Some(end) = after.find('}') {
            let inner = &after[..end];
            if inner.bytes().all(|b| b.is_ascii_digit()) {
                let idx = if inner.is_empty() {
                    next += 1;
                    next - 1
                } else {
                    inner.parse().unwrap_or(0)
                };
                out.push_str(parts.get(idx).map(String::as_str).unwrap_or(""));
                rest = &after[end + 1..];
                continue;
            }
        }
        out.push('{');
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Français → anglais. Les `{}` marquent les parties variables.
pub const EN: &[(&str, &str)] = &[
    // Comptes
    ("Invité", "Guest"),
    (
        "le mot de passe doit faire au moins 8 caractères",
        "the password must be at least 8 characters",
    ),
    ("mot de passe trop long", "password too long"),
    (
        "le prénom doit faire entre 1 et 40 caractères",
        "the first name must be between 1 and 40 characters",
    ),
    ("prénom invalide", "invalid first name"),
    (
        "un compte « {} » existe déjà",
        "an account “{}” already exists",
    ),
    ("compte introuvable", "account not found"),
    (
        "impossible de supprimer le dernier admin",
        "can’t delete the last admin",
    ),
    (
        "prénom ou mot de passe incorrect",
        "wrong first name or password",
    ),
    ("aucun compte connecté", "no account signed in"),
    ("mot de passe actuel incorrect", "wrong current password"),
    (
        "vous ne pouvez pas supprimer votre propre compte",
        "you can’t delete your own account",
    ),
    (
        "cet appareil appartient à un autre compte",
        "this device belongs to another account",
    ),
    ("Connexion requise", "Sign-in required"),
    // Droits
    ("réservé aux comptes {}", "reserved for {} accounts"),
    ("connectés", "signed-in"),
    ("admin", "admin"),
    ("membre ou admin", "member or admin"),
    (
        "cet appareil est en lecture seule pour votre compte",
        "this device is read-only for your account",
    ),
    ("réservé aux membres", "members only"),
    (
        "votre compte ne peut pas modifier ce tableau",
        "your account can’t edit this board",
    ),
    (
        "votre compte peut ajouter des cartes, pas changer les sections",
        "your account can add tiles, not change the sections",
    ),
    (
        "votre compte ne peut pas ajouter de section",
        "your account can’t add a section",
    ),
    (
        "votre compte peut ajouter des cartes, pas modifier les autres",
        "your account can add tiles, not change the others",
    ),
    (
        "votre compte peut modifier les cartes, pas en ajouter",
        "your account can edit tiles, not add any",
    ),
    (
        "requête venue d'un autre site : refusée",
        "request from another site: refused",
    ),
    ("route inconnue", "unknown route"),
    ("{} introuvable", "{} not found"),
    // Tableaux, écrans, réglages
    (
        "il faut au moins un tableau",
        "at least one board is needed",
    ),
    ("{} tableaux au plus", "{} boards at most"),
    (
        "chaque tableau doit avoir un nom",
        "each board needs a name",
    ),
    (
        "au moins un tableau doit rester visible",
        "at least one board must stay visible",
    ),
    (
        "{} sections au plus par tableau",
        "{} sections at most per board",
    ),
    (
        "{} cartes au plus par section",
        "{} tiles at most per section",
    ),
    ("section illisible", "unreadable section"),
    ("cartes illisibles", "unreadable tiles"),
    ("carte illisible", "unreadable tile"),
    ("titre de section invalide", "invalid section title"),
    ("Écran", "Screen"),
    ("écran introuvable", "screen not found"),
    ("donnez un nom à cet écran", "give this screen a name"),
    ("apparence trop volumineuse", "appearance too large"),
    ("chaque pièce doit avoir un nom", "each room needs a name"),
    (
        "la pièce « {} » existe deux fois",
        "the room “{}” exists twice",
    ),
    ("80 pièces au plus", "80 rooms at most"),
    (
        "réglage trop volumineux ({} octets)",
        "setting too large ({} bytes)",
    ),
    ("nom de réglage invalide", "invalid setting name"),
    (
        "ce réglage se modifie par sa propre page",
        "this setting is changed on its own page",
    ),
    ("réglage invalide : {}", "invalid setting: {}"),
    ("lieu invalide : {}", "invalid location: {}"),
    ("maison invalide : {}", "invalid home: {}"),
    (
        "deux groupes s'appellent « {} »",
        "two groups are called “{}”",
    ),
    (
        "« {} » : choisissez au moins deux appareils",
        "“{}”: choose at least two devices",
    ),
    // Appareils
    ("durée de fondu invalide", "invalid fade duration"),
    (
        "réglage « {} » inconnu pour cet appareil",
        "unknown setting “{}” for this device",
    ),
    ("valeur refusée pour « {} »", "value refused for “{}”"),
    ("aucun réglage", "no settings"),
    (
        "action réservée aux appareils Zigbee",
        "action reserved for Zigbee devices",
    ),
    (
        "chaque appareil doit avoir un identifiant",
        "each device needs an identifier",
    ),
    (
        "{} : la clé locale doit faire 16 caractères",
        "{}: the local key must be 16 characters",
    ),
    ("adresse IP « {} » invalide", "invalid IP address “{}”"),
    (
        "version « {} » inconnue (3.3, 3.4 ou 3.5)",
        "unknown version “{}” (3.3, 3.4 or 3.5)",
    ),
    (
        "indiquez l'adresse du serveur Matter",
        "enter the Matter server address",
    ),
    (
        "serveur Matter non configuré",
        "Matter server not configured",
    ),
    (
        "ce n'est pas un niveau de pile",
        "this isn’t a battery level",
    ),
    (
        "cet appareil répond encore : retirez-le d'abord de Zigbee2MQTT, ou masquez-le",
        "this device still responds: remove it from Zigbee2MQTT first, or hide it",
    ),
    ("entité inconnue : {}", "unknown entity: {}"),
    (
        "{} ne se commande pas (capteur)",
        "{} can’t be controlled (sensor)",
    ),
    ("le noyau est arrêté", "the core is stopped"),
    ("durée invalide", "invalid duration"),
    (
        "{} ne répond pas. Vérifiez qu'il est alimenté.",
        "{} isn’t responding. Check that it’s powered.",
    ),
    // Automatismes
    (
        "précisez enabled, state (true, false, « rearm ») ou run",
        "specify enabled, state (true, false, “rearm”) or run",
    ),
    ("activé", "enabled"),
    ("désactivé", "disabled"),
    ("réarmé", "re-armed"),
    ("état mis à {}", "state set to {}"),
    ("vrai", "true"),
    ("faux", "false"),
    ("lancé à la main", "run by hand"),
    ("à la maison", "at home"),
    ("allumé", "on"),
    ("éteint", "off"),
    ("« {} » était {} et non {}", "“{}” was {} and not {}"),
    (
        "« {} » n'a pas encore donné son état",
        "“{}” hasn’t reported its state yet",
    ),
    ("hors de la plage {} – {}", "outside the {} – {} range"),
    ("pas un des jours choisis", "not one of the chosen days"),
    ("il ne faisait pas nuit", "it wasn’t dark"),
    ("ajoutez au moins un « Quand »", "add at least one “When”"),
    ("ajoutez au moins un « Alors »", "add at least one “Then”"),
    ("appareil invalide : {}", "invalid device: {}"),
    (
        "un seuil doit préciser « au-dessus de » ou « en dessous de »",
        "a threshold must specify “above” or “below”",
    ),
    (
        "heure invalide : {} (format 07:30)",
        "invalid time: {} (format 07:30)",
    ),
    (
        "décalage trop grand (6 h au plus)",
        "offset too large (6 h at most)",
    ),
    (
        "heure de réarmement invalide : {}",
        "invalid re-arm time: {}",
    ),
    ("délai de réarmement invalide", "invalid re-arm delay"),
    ("heure invalide : {}", "invalid time: {}"),
    ("le message est vide", "the message is empty"),
    ("moteur d'automatismes arrêté", "automation engine stopped"),
    ("réarmement automatique", "automatic re-arm"),
    (
        "automatisme « {} » : état {} ({})",
        "automation “{}”: state {} ({})",
    ),
    ("identifiant invalide", "invalid identifier"),
    ("aucun destinataire", "no recipients"),
    (
        "notification non envoyée : aucun téléphone n'a encore ouvert Heim",
        "notification not sent: no phone has opened Heim yet",
    ),
    ("Automatisme : {}", "Automation: {}"),
    // Scènes
    ("moteur de scènes arrêté", "scene engine stopped"),
    (
        "une scène doit contenir au moins un appareil",
        "a scene must contain at least one device",
    ),
    (
        "60 appareils au plus dans une scène",
        "60 devices at most in a scene",
    ),
    (
        "fondu invalide (60 secondes au plus)",
        "invalid fade (60 seconds at most)",
    ),
    ("couleur de la scène invalide", "invalid scene colour"),
    ("icône invalide", "invalid icon"),
    (
        "{} : seules les lumières, les prises et les groupes peuvent faire partie d'une scène",
        "{}: only lights, plugs and groups can be part of a scene",
    ),
    (
        "{} : couleur invalide (attendu #rrggbb)",
        "{}: invalid colour (expected #rrggbb)",
    ),
    ("{} : effet invalide", "{}: invalid effect"),
    ("identifiant de scène invalide", "invalid scene identifier"),
    ("aperçu de scène", "scene preview"),
    (
        "{} est injoignable ou inconnue, impossible de la capturer",
        "{} is unreachable or unknown, it can’t be captured",
    ),
    ("scène inconnue : {}", "unknown scene: {}"),
    (
        "durée invalide (24 h au plus)",
        "invalid duration (24 h at most)",
    ),
    ("la scène doit avoir un nom", "the scene needs a name"),
    ("trop d'appareils", "too many devices"),
    // Minuteurs
    ("moteur de timers arrêté", "timer engine stopped"),
    (
        "durée invalide : {} s (entre 1 s et 7 jours)",
        "invalid duration: {} s (between 1 s and 7 days)",
    ),
    ("appareil inconnu : {}", "unknown device: {}"),
    ("Arrêt auto de {}", "Auto-off for {}"),
    ("le timer doit avoir un nom", "the timer needs a name"),
    ("identifiant de timer invalide", "invalid timer identifier"),
    ("entité d'action invalide : {}", "invalid action entity: {}"),
    ("Minuteur terminé", "Timer finished"),
    ("« {} » est terminé.", "“{}” has finished."),
    // Journal
    ("pile changée", "battery changed"),
    (
        "redémarrage du Raspberry Pi demandé",
        "Raspberry Pi restart requested",
    ),
    (
        "arrêt du Raspberry Pi demandé",
        "Raspberry Pi shutdown requested",
    ),
    ("scène {}", "scene {}"),
    ("lancé", "ran"),
    ("lancé, avec une erreur : {}", "ran, with an error: {}"),
    ("lancé, avec une erreur", "ran, with an error"),
    ("pas lancé : {}", "not run: {}"),
    ("par « {} »", "by “{}”"),
    ("ne répond plus", "no longer responds"),
    ("répond de nouveau", "responds again"),
    ("Heim {} démarré", "Heim {} started"),
    (
        "{} : après coupure de courant réglé sur « {} »",
        "{}: after a power cut set to “{}”",
    ),
    ("fermé", "closed"),
    // Alertes et notifications
    ("Le serveur Matter", "The Matter server"),
    (
        "{} : {} % de pile. Pensez à la changer.",
        "{}: {} % battery. Remember to change it.",
    ),
    ("{} détecte de l'eau !", "{} detects water!"),
    ("La connexion est revenue.", "The connection is back."),
    (
        "Heim n'arrive plus à joindre {} depuis 5 minutes : les appareils concernés ne répondent plus.",
        "Heim hasn’t been able to reach {} for 5 minutes: the devices concerned no longer respond.",
    ),
    ("Appareil muet", "Silent device"),
    ("Pile faible", "Low battery"),
    ("Fuite d'eau", "Water leak"),
    ("Mise à jour disponible", "Update available"),
    (
        "Heim {} est disponible : Réglages > Système.",
        "Heim {} is available: Settings > System.",
    ),
    ("la cible doit avoir un nom", "the target needs a name"),
    ("identifiant de cible invalide", "invalid target identifier"),
    (
        "l'URL du webhook doit commencer par http:// ou https://",
        "the webhook URL must start with http:// or https://",
    ),
    ("aucun destinataire ne correspond", "no recipient matches"),
    (
        "appareil inconnu : rechargez la page",
        "unknown device: reload the page",
    ),
    (
        "seule l'application peut recevoir des notifications push",
        "only the app can receive push notifications",
    ),
    (
        "cet appareil n'est pas abonné aux notifications",
        "this device isn’t subscribed to notifications",
    ),
    ("clés push absentes", "push keys missing"),
    (
        "Les notifications marchent sur cet appareil.",
        "Notifications work on this device.",
    ),
    (
        "l'abonnement n'est plus valable : réactivez les notifications",
        "the subscription is no longer valid: enable notifications again",
    ),
    (
        "Heim compilé sans accès HTTP",
        "Heim built without HTTP access",
    ),
    (
        "adresse d'abonnement invalide",
        "invalid subscription address",
    ),
    ("clé p256dh illisible", "unreadable p256dh key"),
    ("clé auth illisible", "unreadable auth key"),
    ("clés d'abonnement invalides", "invalid subscription keys"),
    (
        "message trop long pour une notification",
        "message too long for a notification",
    ),
    ("clé p256dh invalide", "invalid p256dh key"),
    ("nonce invalide", "invalid nonce"),
    (
        "service push : réponse {} {}",
        "push service: response {} {}",
    ),
    // Présence
    (
        "le compte « {} » n'existe pas",
        "the account “{}” doesn’t exist",
    ),
    (
        "délai d'absence : entre 1 et 120 minutes",
        "away delay: between 1 and 120 minutes",
    ),
    ("adresse « {} » invalide", "invalid address “{}”"),
    ("Tout le monde est parti", "Everyone has left"),
    (
        "La maison est vide. Touchez pour activer l'alarme.",
        "The house is empty. Tap to arm the alarm.",
    ),
    ("Alarme activée", "Alarm armed"),
    (
        "Tout le monde est parti : l'alarme passe en mode Absent.",
        "Everyone has left: the alarm switches to Away mode.",
    ),
    (
        "L'alarme n'a pas pu s'activer ({}). Touchez pour vérifier.",
        "The alarm couldn’t be armed ({}). Tap to check.",
    ),
    ("Retour à la maison", "Back home"),
    (
        "{} est rentré(e). L'alarme est toujours activée.",
        "{} is back home. The alarm is still armed.",
    ),
    // Alarme
    ("code actuel erroné", "wrong current code"),
    (
        "Zigbee2MQTT n'est pas connecté : vérifiez Réglages > Connexion Zigbee",
        "Zigbee2MQTT isn’t connected: check Settings > Zigbee connection",
    ),
    (
        "les délais sont limités à 10 minutes",
        "delays are limited to 10 minutes",
    ),
    (
        "durée des sirènes : entre 5 secondes et 15 minutes",
        "siren duration: between 5 seconds and 15 minutes",
    ),
    (
        "chaque caméra doit avoir un nom",
        "each camera needs a name",
    ),
    (
        "caméra « {} » : l'adresse doit commencer par http:// ou https://",
        "camera “{}”: the address must start with http:// or https://",
    ),
    (
        "le code doit faire entre 4 et 12 caractères",
        "the code must be between 4 and 12 characters",
    ),
    ("aucune sirène choisie", "no siren chosen"),
    ("Essai des sirènes ({} s)", "Siren test ({} s)"),
    ("aucune caméra", "no camera"),
    (
        "trop d'essais : patientez une minute",
        "too many attempts: wait a minute",
    ),
    ("Code erroné", "Wrong code"),
    ("code demandé", "code required"),
    ("code erroné", "wrong code"),
    (
        "aucun capteur surveillé en mode {} : choisissez-les dans Réglages > Sécurité",
        "no sensor watched in {} mode: choose them in Settings > Security",
    ),
    ("Absent", "Away"),
    ("Nuit", "Night"),
    ("Activation {} dans {} s", "Arming {} in {} s"),
    (
        "Alarme activée : {} (ignorés : {})",
        "Alarm armed: {} (ignored: {})",
    ),
    ("Alarme activée : {}", "Alarm armed: {}"),
    ("Alarme arrêtée", "Alarm stopped"),
    ("Alarme désactivée", "Alarm disarmed"),
    ("Resté ouvert, ignoré : {}", "Left open, ignored: {}"),
    (
        "Attention, resté ouvert (non surveillé) : {}",
        "Careful, left open (not watched): {}",
    ),
    ("délai d'entrée écoulé", "entry delay over"),
    ("{} ne répond plus", "{} no longer responds"),
    (
        "{} ne répond plus : il n'est plus surveillé.",
        "{} no longer responds: it’s no longer watched.",
    ),
    ("{} : {} s pour désactiver", "{}: {} s to disarm"),
    ("{} s'est déclenché.", "{} went off."),
    ("Alarme déclenchée", "Alarm triggered"),
    ("réponse {}", "response {}"),
    ("ce n'est pas une image JPEG", "this isn’t a JPEG image"),
    (
        "Heim a été compilé sans accès HTTP (fonction webhook)",
        "Heim was built without HTTP access (webhook feature)",
    ),
    // Réseau et premier démarrage
    ("étape inconnue", "unknown step"),
    (
        "le réseau se règle sur la machine elle-même (Heim ne le gère pas ici)",
        "the network is set on the machine itself (Heim doesn’t manage it here)",
    ),
    ("aucun câble réseau connecté", "no network cable connected"),
    ("{} introuvable : {}", "{} not found: {}"),
    (
        "le nom se règle sur la machine elle-même",
        "the name is set on the machine itself",
    ),
    (
        "nom invalide : lettres minuscules, chiffres et tirets",
        "invalid name: lowercase letters, digits and hyphens",
    ),
    (
        "le fuseau horaire se règle sur la machine elle-même",
        "the time zone is set on the machine itself",
    ),
    ("fuseau horaire inconnu", "unknown time zone"),
    (
        "le Wi-Fi se règle sur la machine elle-même (Heim ne le gère pas ici)",
        "Wi-Fi is set on the machine itself (Heim doesn’t manage it here)",
    ),
    ("choisissez un réseau Wi-Fi", "choose a Wi-Fi network"),
    ("nom de réseau Wi-Fi invalide", "invalid Wi-Fi network name"),
    (
        "mot de passe Wi-Fi invalide (63 caractères au plus)",
        "invalid Wi-Fi password (63 characters at most)",
    ),
    (
        "un mot de passe Wi-Fi fait au moins 8 caractères",
        "a Wi-Fi password is at least 8 characters",
    ),
    (
        "créez d'abord le compte administrateur",
        "create the administrator account first",
    ),
    (
        "adresse IP invalide (ex. 192.168.1.50)",
        "invalid IP address (e.g. 192.168.1.50)",
    ),
    ("masque invalide (ex. /24)", "invalid mask (e.g. /24)"),
    (
        "passerelle invalide (ex. 192.168.1.1)",
        "invalid gateway (e.g. 192.168.1.1)",
    ),
    (
        "serveur DNS invalide (ex. 1.1.1.1)",
        "invalid DNS server (e.g. 1.1.1.1)",
    ),
    ("mot de passe refusé par « {} »", "password refused by “{}”"),
    ("réseau « {} » introuvable", "network “{}” not found"),
    (
        "connexion à « {} » impossible : {}",
        "can’t connect to “{}”: {}",
    ),
    // Performances
    ("Box Heim", "Heim box"),
    ("Mémoire", "Memory"),
    ("Processeur", "Processor"),
    ("Carte SD", "SD card"),
    ("Température du processeur", "Processor temperature"),
    // Connexions (Zigbee, Matter, Tuya)
    ("port invalide", "invalid port"),
    (
        "chaque compte MQTT doit avoir un nom (64 caractères au plus)",
        "each MQTT account needs a name (64 characters at most)",
    ),
    (
        "mot de passe MQTT trop court (8 caractères au moins)",
        "MQTT password too short (at least 8 characters)",
    ),
    (
        "le compte MQTT « {} » existe deux fois",
        "the MQTT account “{}” exists twice",
    ),
    (
        "le compte MQTT « {} » existe déjà",
        "the MQTT account “{}” already exists",
    ),
    ("20 comptes MQTT au plus", "20 MQTT accounts at most"),
    (
        "le port {} est déjà pris (un autre broker, Mosquitto ?)",
        "port {} is already taken (another broker, Mosquitto?)",
    ),
    ("impossible d'écouter sur {} : {}", "can’t listen on {}: {}"),
    (
        "Zigbee2MQTT sur la box utilise le broker : arrêtez d'abord Zigbee sur la box",
        "Zigbee2MQTT on the box uses the broker: stop Zigbee on the box first",
    ),
    (
        "Zigbee2MQTT n'est pas installé sur cette box",
        "Zigbee2MQTT isn’t installed on this box",
    ),
    ("aucune clé Zigbee branchée", "no Zigbee stick plugged in"),
    (
        "Zigbee2MQTT n'est pas activé sur la box",
        "Zigbee2MQTT isn’t enabled on the box",
    ),
    (
        "Zigbee2MQTT repris d'une autre installation ({} appareil(s))",
        "Zigbee2MQTT taken over from another installation ({} device(s))",
    ),
    (
        "configuration.yaml : ce n'est pas une configuration",
        "configuration.yaml: this isn’t a configuration",
    ),
    (
        "configuration.yaml illisible : {}",
        "configuration.yaml can’t be read: {}",
    ),
    (
        "configuration.yaml illisible",
        "configuration.yaml can’t be read",
    ),
    ("pilote de clé inconnu", "unknown stick driver"),
    (
        "coordinator_backup.json illisible",
        "coordinator_backup.json can’t be read",
    ),
    (
        "database.db illisible (une ligne JSON par appareil attendue)",
        "database.db can’t be read (one JSON line per device expected)",
    ),
    (
        "impossible de piloter Zigbee2MQTT : {}",
        "can’t control Zigbee2MQTT: {}",
    ),
    ("Zigbee2MQTT est arrêté", "Zigbee2MQTT is stopped"),
    (
        "broker joint, mais Zigbee2MQTT ne répond pas",
        "broker reached, but Zigbee2MQTT isn’t answering",
    ),
    (
        "Le broker intégré : Zigbee2MQTT, Tasmota, Shelly…",
        "The built-in broker: Zigbee2MQTT, Tasmota, Shelly…",
    ),
    ("Zigbee2MQTT sur la box", "Zigbee2MQTT on the box"),
    ("Sur la box", "On the box"),
    ("port {} · {} client(s)", "port {} · {} client(s)"),
    (
        "indiquez l'adresse du broker MQTT (l'IP de la machine Home Assistant ou Mosquitto)",
        "enter the MQTT broker address (the IP of the Home Assistant or Mosquitto machine)",
    ),
    (
        "indiquez d'abord l'adresse du broker MQTT",
        "enter the MQTT broker address first",
    ),
    (
        "indiquez d'abord l'adresse du serveur Matter",
        "enter the Matter server address first",
    ),
    ("{} appareil(s) déclaré(s)", "{} device(s) declared"),
    (
        "Par le serveur Matter de Home Assistant",
        "Through Home Assistant’s Matter server",
    ),
    (
        "Par Zigbee2MQTT et son broker MQTT",
        "Through Zigbee2MQTT and its MQTT broker",
    ),
    (
        "Prises, ampoules Wi-Fi en local (LSC, Lidl…)",
        "Wi-Fi plugs and bulbs, locally (LSC, Lidl…)",
    ),
    (
        "Appareils Zigbee par Zigbee2MQTT",
        "Zigbee devices through Zigbee2MQTT",
    ),
    ("{} appareil(s)", "{} device(s)"),
    ("{} appareil(s), SDK {}", "{} device(s), SDK {}"),
    ("{} ne répond pas", "{} isn’t responding"),
    ("connexion à {} impossible : {}", "can’t connect to {}: {}"),
    (
        "le serveur Matter ne s'est pas présenté",
        "the Matter server didn’t introduce itself",
    ),
    (
        "le serveur Matter n'a pas répondu à temps",
        "the Matter server didn’t answer in time",
    ),
    (
        "connexion au serveur Matter coupée",
        "connection to the Matter server lost",
    ),
    (
        "ce fichier ne contient pas de liste d'appareils",
        "this file doesn’t contain a device list",
    ),
    (
        "aucun appareil utilisable dans ce fichier (il faut « id » et « key »)",
        "no usable device in this file (“id” and “key” are needed)",
    ),
    (
        "{} refuse la connexion ({}) — déjà connecté à une autre application ?",
        "{} refuses the connection ({}) — already connected to another app?",
    ),
    (
        "pas de réponse à la négociation (version ou clé incorrecte ?)",
        "no answer to the handshake (wrong version or key?)",
    ),
    ("connexion coupée : {}", "connection lost: {}"),
    (
        "l'appareil a fermé la connexion",
        "the device closed the connection",
    ),
    (
        "longueur chiffrée invalide ({})",
        "invalid encrypted length ({})",
    ),
    (
        "vecteur d'initialisation invalide",
        "invalid initialisation vector",
    ),
    (
        "message illisible (mauvaise clé locale ?)",
        "unreadable message (wrong local key?)",
    ),
    (
        "la clé locale doit faire 16 caractères (celle-ci en fait {})",
        "the local key must be 16 characters (this one has {})",
    ),
    (
        "l'appareil refuse la connexion (clé locale incorrecte ?)",
        "the device refuses the connection (wrong local key?)",
    ),
    ("clé locale incorrecte", "wrong local key"),
    (
        "trame trop grande : flux désynchronisé",
        "frame too large: stream out of sync",
    ),
    (
        "signature invalide (clé de session ?)",
        "invalid signature (session key?)",
    ),
    ("trame 3.5 invalide", "invalid 3.5 frame"),
    ("message illisible : {}", "unreadable message: {}"),
    // Sauvegardes, système, mises à jour
    (
        "ce fichier n'est pas une sauvegarde Heim",
        "this file isn’t a Heim backup",
    ),
    (
        "Sauvegarde prête : redémarrez Heim pour la charger.",
        "Backup ready: restart Heim to load it.",
    ),
    (
        "dépôt attendu sous la forme proprietaire/nom (exemple : alex/heim)",
        "repository expected as owner/name (example: alex/heim)",
    ),
    (
        "Heim ne tourne pas comme service : il ne redémarrerait pas tout seul",
        "Heim isn’t running as a service: it wouldn’t restart by itself",
    ),
    ("stockage arrêté", "storage stopped"),
    ("dossier des sauvegardes : {}", "backup folder: {}"),
    (
        "seulement sur Raspberry Pi / Linux",
        "only on Raspberry Pi / Linux",
    ),
    ("refusé par le système : {}", "refused by the system: {}"),
    (
        "installation automatique seulement sur Raspberry Pi / Linux : téléchargez la nouvelle version",
        "automatic installation only on Raspberry Pi / Linux: download the new version",
    ),
    (
        "pas de version publiée pour ce processeur",
        "no published version for this processor",
    ),
    (
        "Heim doit tourner comme service (systemd) pour redémarrer tout seul",
        "Heim must run as a service (systemd) to restart by itself",
    ),
    (
        "pas le droit d'écrire dans {}",
        "not allowed to write in {}",
    ),
    (
        "la version {} n'a pas réussi à démarrer : Heim est revenu à la {}, avec vos données d'avant",
        "version {} failed to start: Heim went back to {}, with your previous data",
    ),
    (
        "indiquez le dépôt GitHub (exemple : alex/heim)",
        "enter the GitHub repository (example: alex/heim)",
    ),
    (
        "dépôt « {} » introuvable (il doit être public)",
        "repository “{}” not found (it must be public)",
    ),
    (
        "GitHub limite les vérifications : réessayez dans une heure",
        "GitHub is limiting checks: try again in an hour",
    ),
    ("GitHub répond {}", "GitHub answers {}"),
    (
        "aucune version publiée pour l'instant",
        "no published version yet",
    ),
    (
        "pas de fichier pour cette machine dans la release",
        "no file for this machine in the release",
    ),
    (
        "fichier SHA256SUMS absent de la release",
        "SHA256SUMS file missing from the release",
    ),
    ("téléchargement impossible : {}", "download failed: {}"),
    ("téléchargement : réponse {}", "download: response {}"),
    (
        "le fichier téléchargé est abîmé (empreinte différente) : rien n'a été changé",
        "the downloaded file is damaged (different checksum): nothing was changed",
    ),
    (
        "le fichier téléchargé n'est pas un programme Linux",
        "the downloaded file isn’t a Linux program",
    ),
    ("écriture impossible : {}", "can’t write: {}"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_with_holes() {
        assert_eq!(tr("en", "prénom invalide"), "invalid first name");
        assert_eq!(
            tr("en", "réseau « Livebox » introuvable"),
            "network “Livebox” not found"
        );
        assert_eq!(tr("en", "Alarme activée : Absent"), "Alarm armed: Away");
        assert_eq!(
            tr("en", "réservé aux comptes connectés"),
            "reserved for signed-in accounts"
        );
        assert_eq!(
            tr("en", "Lampe salon : 12 % de pile. Pensez à la changer."),
            "Lampe salon: 12 % battery. Remember to change it."
        );
        assert_eq!(tr("en", "texte inconnu"), "texte inconnu");
        assert_eq!(tr("fr", "prénom invalide"), "prénom invalide");
        assert_eq!(known(Some("en-GB")), Some("en"));
        assert_eq!(known(Some("de")), None);
    }

    #[test]
    fn every_pattern_has_the_same_holes() {
        for (fr, en) in EN {
            let holes = fr.matches("{}").count();
            let used = en.matches("{}").count()
                + (0..holes)
                    .filter(|i| en.contains(&format!("{{{i}}}")))
                    .count();
            assert_eq!(holes, used, "« {fr} »");
        }
    }
}
