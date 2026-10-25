# Sécurité de Heim

## Signaler une faille

Merci de ne pas ouvrir d'issue publique pour une faille. Utilisez l'onglet
**Security › Report a vulnerability** du dépôt GitHub (signalement privé). Une réponse est
donnée dès que possible, et la correction sort dans une version publiée par les releases.

## Ce que Heim protège, et comment

| Sujet | Protection |
| --- | --- |
| Comptes | Mots de passe hachés (PBKDF2-SHA256, 100 000 tours, sel par compte), 8 caractères au moins, comparaison en temps constant. Au-delà de 10 échecs en 5 minutes, les connexions sont refusées un moment. |
| Sessions | Jeton aléatoire de 256 bits dans un cookie `HttpOnly; SameSite=Lax` (`Secure` en HTTPS). La base ne garde que son empreinte SHA-256 : une base volée n'ouvre aucune session. Un an au plus. |
| Rôles | Chaque route de l'API exige un rôle (invité, membre, admin) ; ce qui n'est pas listé est réservé à l'admin. Un invité ne commande ni les sirènes, ni l'alarme, ni l'état des automatismes. |
| Autres sites | Une requête qui modifie quelque chose, ou qui ouvre le WebSocket, venue d'une autre page que Heim (même sur la même machine, comme Home Assistant) est refusée. |
| Navigateur | Politique de contenu stricte (le seul script autorisé est celui de l'interface, par son empreinte), pas d'affichage dans un cadre, `nosniff`, aucune adresse transmise aux autres sites, réponses de l'API jamais mises en cache. |
| Secrets | Le mot de passe MQTT, les clés locales Tuya, le code de l'alarme et la clé privée des notifications ne sortent jamais par l'API. Les réglages internes ne s'écrivent que par leurs propres routes. |
| Notifications | Heim n'envoie qu'aux vrais services push (Apple, Google, Mozilla, Microsoft) ; un compte ne touche qu'aux notifications de ses propres appareils. |
| Base de données | SQLite, fichier local jamais exposé sur le réseau, dossier en `700` et fichiers en `600`. Toutes les requêtes SQL passent des paramètres liés (pas d'injection). SQLite n'a pas de « RLS » à la Supabase : le contrôle d'accès est fait par l'API, route par route. |
| Commandes système | `nmcli`, `ping`, `systemctl`, `tailscale` sont lancés avec des arguments séparés, jamais par un shell. |
| Service | Utilisateur système `heim` sans shell, `NoNewPrivileges`, système de fichiers en lecture seule sauf ses deux dossiers, noyau et cgroups protégés, `UMask=0077`, mémoire limitée. Polkit ne lui permet que le Wi-Fi et l'arrêt/redémarrage. |
| Mises à jour | Téléchargées depuis les releases GitHub en HTTPS, vérifiées par SHA-256, avec retour automatique à l'ancienne version si la nouvelle ne démarre pas. |
| Bibliothèques | `cargo audit` et `npm audit` à chaque push et chaque lundi ; Dependabot propose les mises à jour chaque semaine. |

## À savoir

- **Sans compte, Heim est ouvert** à tout le réseau local (comme avant la 0.5). L'assistant du
  premier démarrage crée un compte : gardez-en au moins un.
- **Accès depuis dehors** : seulement par Tailscale (HTTPS, aucun port ouvert sur la box).
  N'ouvrez pas le port de Heim sur Internet.
- **MQTT** : Heim parle au broker sans chiffrement, comme Home Assistant par défaut. Il doit
  rester sur le réseau de la maison.
- **Sauvegardes** : un fichier de sauvegarde contient tout, y compris les clés Tuya et le mot de
  passe MQTT. Gardez-le pour vous (jamais dans un dépôt public).
- **Compte console du Pi** (`heim-admin`) : SSH est coupé. Définissez le secret `PI_PASSWORD`
  dans le dépôt pour remplacer le mot de passe par défaut des images.
