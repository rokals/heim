//! Tableaux de bord : plusieurs pages d'accueil, chacune avec ses sections et ses cartes.
//!
//! Le contenu des cartes appartient à l'interface (taille, couleurs, règles…) : le noyau le
//! range tel quel, vérifie sa forme et sa taille, et décide qui a le droit d'y toucher.
//! - un admin fait tout ;
//! - un membre ou un invité voit les tableaux non cachés, ou seulement celui qu'on lui impose ;
//!   il peut modifier et/ou ajouter des cartes si son compte l'autorise.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::accounts::{Access, Role};
use crate::model::slugify;
use crate::settings::Settings;

pub const KEY: &str = "dashboards";
/// Le tableau unique d'avant la 0.12.
const LEGACY: &str = "ui.dashboard";
const MAX_BOARDS: usize = 20;
const MAX_SECTIONS: usize = 40;
const MAX_CARDS: usize = 150;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Board {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Absent de la navigation (un tableau pour la tablette de la cuisine).
    #[serde(default)]
    pub hidden: bool,
    /// Rangé tout seul par pièce, sans sections à soi.
    #[serde(default)]
    pub auto: bool,
    #[serde(default)]
    pub sections: Vec<Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Boards {
    #[serde(default)]
    pub boards: Vec<Board>,
}

/// Une section « Maison » en tête des anciens tableaux : on y retrouve « Éteindre les
/// lumières » et les scènes, qui étaient affichés en haut de l'accueil.
fn house_section() -> Value {
    json!({
        "title": "",
        "cards": [
            { "type": "house", "kind": "lights", "w": 2, "h": 1 },
            { "type": "house", "kind": "plugs", "w": 2, "h": 1 },
            { "type": "house", "kind": "scenes", "w": 4, "h": 1 },
        ]
    })
}

fn default_boards(legacy: Option<Value>) -> Boards {
    let sections = legacy
        .as_ref()
        .and_then(|v| v.get("sections"))
        .and_then(Value::as_array)
        .filter(|s| !s.is_empty())
        .cloned();
    let board = match sections {
        Some(mut s) => {
            s.insert(0, house_section());
            Board {
                id: "maison".into(),
                name: "Maison".into(),
                icon: Some("house".into()),
                hidden: false,
                auto: false,
                sections: s,
            }
        }
        None => Board {
            id: "maison".into(),
            name: "Maison".into(),
            icon: Some("house".into()),
            hidden: false,
            auto: true,
            sections: Vec::new(),
        },
    };
    Boards {
        boards: vec![board],
    }
}

/// Les tableaux, ou ceux d'avant (un seul) tant que rien n'a été enregistré.
pub fn load(settings: &Settings) -> Boards {
    match settings.get::<Boards>(KEY) {
        Some(b) if !b.boards.is_empty() => b,
        _ => default_boards(settings.get_raw(LEGACY)),
    }
}

fn check_sections(sections: &[Value]) -> Result<(), String> {
    if sections.len() > MAX_SECTIONS {
        return Err(format!("{MAX_SECTIONS} sections au plus par tableau"));
    }
    for s in sections {
        let obj = s.as_object().ok_or("section illisible")?;
        if obj
            .get("title")
            .is_some_and(|t| !t.is_string() || t.as_str().is_some_and(|t| t.chars().count() > 80))
        {
            return Err("titre de section invalide".into());
        }
        let cards = obj
            .get("cards")
            .map(|c| c.as_array().ok_or("cartes illisibles"))
            .transpose()?
            .map(Vec::len)
            .unwrap_or(0);
        if cards > MAX_CARDS {
            return Err(format!("{MAX_CARDS} cartes au plus par section"));
        }
        if let Some(cards) = obj.get("cards").and_then(Value::as_array)
            && cards.iter().any(|c| !c.is_object())
        {
            return Err("carte illisible".into());
        }
    }
    Ok(())
}

/// Remet de l'ordre : noms, identifiants uniques, formes des sections.
pub fn validate(mut input: Boards) -> Result<Boards, String> {
    if input.boards.is_empty() {
        return Err("il faut au moins un tableau".into());
    }
    if input.boards.len() > MAX_BOARDS {
        return Err(format!("{MAX_BOARDS} tableaux au plus"));
    }
    let mut seen = std::collections::HashSet::new();
    for b in &mut input.boards {
        b.name = b.name.trim().chars().take(40).collect();
        if b.name.is_empty() {
            return Err("chaque tableau doit avoir un nom".into());
        }
        let mut id = if b.id.trim().is_empty() {
            slugify(&b.name)
        } else {
            slugify(&b.id)
        };
        if id.is_empty() {
            id = "tableau".into();
        }
        let base = id.clone();
        let mut n = 2;
        while !seen.insert(id.clone()) {
            id = format!("{base}_{n}");
            n += 1;
        }
        b.id = id;
        b.icon = b
            .icon
            .take()
            .map(|i| i.chars().take(24).collect::<String>())
            .filter(|i| !i.is_empty());
        check_sections(&b.sections)?;
    }
    if input.boards.iter().all(|b| b.hidden) {
        return Err("au moins un tableau doit rester visible".into());
    }
    Ok(input)
}

/// Ce qu'un compte voit des tableaux et ce qu'il peut en faire.
#[derive(Debug, Clone, Serialize)]
pub struct View {
    pub boards: Vec<Board>,
    /// Le tableau qui s'ouvre (imposé, sinon le premier visible).
    pub home: Option<String>,
    /// Un tableau imposé : pas de navigation entre tableaux.
    pub locked: bool,
    pub can_edit: bool,
    pub can_add: bool,
    /// Créer, renommer, cacher, supprimer des tableaux (admin).
    pub can_manage: bool,
}

pub fn view(all: &Boards, role: Role, access: &Access) -> View {
    if role == Role::Admin {
        return View {
            home: all.boards.iter().find(|b| !b.hidden).map(|b| b.id.clone()),
            boards: all.boards.clone(),
            locked: false,
            can_edit: true,
            can_add: true,
            can_manage: true,
        };
    }
    let imposed = access
        .board
        .as_ref()
        .and_then(|id| all.boards.iter().find(|b| &b.id == id));
    let boards: Vec<Board> = match imposed {
        Some(b) => vec![b.clone()],
        None => all.boards.iter().filter(|b| !b.hidden).cloned().collect(),
    };
    View {
        home: boards.first().map(|b| b.id.clone()),
        boards,
        locked: imposed.is_some(),
        can_edit: access.can_edit,
        can_add: access.can_add,
        can_manage: false,
    }
}

/// Ce qui identifie une carte quand on vérifie un ajout : l'appareil, ou le genre de carte.
fn card_key(card: &Value) -> String {
    if let Some(id) = card.get("entity_id").and_then(Value::as_str) {
        return id.to_string();
    }
    let t = card.get("type").and_then(Value::as_str).unwrap_or("?");
    let kind = card.get("kind").and_then(Value::as_str).unwrap_or("");
    format!("{t}:{kind}")
}

fn cards(section: &Value) -> Vec<Value> {
    section
        .get("cards")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn title(section: &Value) -> Option<&str> {
    section.get("title").and_then(Value::as_str)
}

/// `old` est-il une sous-suite de `new` (mêmes cartes, même ordre, d'autres en plus) ?
fn is_subsequence(old: &[Value], new: &[Value]) -> bool {
    let mut it = new.iter();
    old.iter().all(|o| it.any(|n| n == o))
}

/// Vérifie qu'un compte sans tous les droits n'a fait que ce qu'il a le droit de faire.
/// - modifier sans ajouter : pas de nouvelle carte (même appareil en plus = ajout) ;
/// - ajouter sans modifier : les cartes d'avant restent telles quelles, dans le même ordre.
pub fn check_change(
    old: &[Value],
    new: &[Value],
    can_edit: bool,
    can_add: bool,
) -> Result<(), String> {
    if can_edit && can_add {
        return Ok(());
    }
    if !can_edit && !can_add {
        return Err("votre compte ne peut pas modifier ce tableau".into());
    }
    if old.len() != new.len() || old.iter().zip(new).any(|(a, b)| title(a) != title(b)) {
        if !can_edit {
            return Err("votre compte peut ajouter des cartes, pas changer les sections".into());
        }
        if new.len() > old.len() {
            return Err("votre compte ne peut pas ajouter de section".into());
        }
    }
    if !can_edit {
        for (a, b) in old.iter().zip(new) {
            if !is_subsequence(&cards(a), &cards(b)) {
                return Err("votre compte peut ajouter des cartes, pas modifier les autres".into());
            }
        }
        return Ok(());
    }
    // Modifier sans ajouter : chaque carte d'après existait déjà quelque part.
    let mut pool: Vec<String> = old.iter().flat_map(cards).map(|c| card_key(&c)).collect();
    for c in new.iter().flat_map(cards) {
        let k = card_key(&c);
        match pool.iter().position(|p| *p == k) {
            Some(i) => {
                pool.swap_remove(i);
            }
            None => return Err("votre compte peut modifier les cartes, pas en ajouter".into()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sec(title: &str, cards: Value) -> Value {
        json!({ "title": title, "cards": cards })
    }

    #[test]
    fn old_dashboard_becomes_first_board_with_house_cards() {
        let b = default_boards(Some(
            json!({ "sections": [sec("Salon", json!([{ "entity_id": "light.a" }]))] }),
        ));
        assert_eq!(b.boards.len(), 1);
        assert!(!b.boards[0].auto);
        assert_eq!(b.boards[0].sections.len(), 2);
        assert_eq!(b.boards[0].sections[0]["title"], "");
        let empty = default_boards(None);
        assert!(empty.boards[0].auto);
    }

    #[test]
    fn validate_makes_unique_ids() {
        let b = validate(Boards {
            boards: vec![
                Board {
                    id: String::new(),
                    name: "Cuisine".into(),
                    icon: None,
                    hidden: false,
                    auto: false,
                    sections: vec![],
                },
                Board {
                    id: String::new(),
                    name: "Cuisine".into(),
                    icon: None,
                    hidden: true,
                    auto: false,
                    sections: vec![],
                },
            ],
        })
        .unwrap();
        assert_eq!(b.boards[0].id, "cuisine");
        assert_eq!(b.boards[1].id, "cuisine_2");
        let all_hidden = Boards {
            boards: vec![Board {
                id: "a".into(),
                name: "A".into(),
                icon: None,
                hidden: true,
                auto: false,
                sections: vec![],
            }],
        };
        assert!(validate(all_hidden).is_err());
    }

    #[test]
    fn imposed_board_and_hidden_boards() {
        let all = Boards {
            boards: vec![
                Board {
                    id: "maison".into(),
                    name: "Maison".into(),
                    icon: None,
                    hidden: false,
                    auto: true,
                    sections: vec![],
                },
                Board {
                    id: "cuisine".into(),
                    name: "Cuisine".into(),
                    icon: None,
                    hidden: true,
                    auto: false,
                    sections: vec![],
                },
            ],
        };
        let free = view(&all, Role::Member, &Access::default());
        assert_eq!(free.boards.len(), 1);
        assert!(!free.locked && !free.can_edit);
        let tablet = view(
            &all,
            Role::Member,
            &Access {
                board: Some("cuisine".into()),
                ..Default::default()
            },
        );
        assert_eq!(tablet.boards[0].id, "cuisine");
        assert!(tablet.locked);
        assert_eq!(view(&all, Role::Admin, &Access::default()).boards.len(), 2);
    }

    #[test]
    fn change_rules() {
        let a = json!({ "entity_id": "light.a", "w": 2, "h": 2 });
        let b = json!({ "entity_id": "light.b" });
        let old = vec![sec("Salon", json!([a.clone()]))];
        let added = vec![sec("Salon", json!([a.clone(), b.clone()]))];
        let resized = vec![sec(
            "Salon",
            json!([{ "entity_id": "light.a", "w": 4, "h": 1 }]),
        )];
        assert!(check_change(&old, &added, false, true).is_ok());
        assert!(check_change(&old, &resized, false, true).is_err());
        assert!(check_change(&old, &resized, true, false).is_ok());
        assert!(check_change(&old, &added, true, false).is_err());
        assert!(
            check_change(&old, &[sec("Salon", json!([]))], true, false).is_ok(),
            "retirer = modifier"
        );
        assert!(check_change(&old, &added, false, false).is_err());
    }
}
