# Heim security

[Français](SECURITY.fr.md)

## Reporting a vulnerability

Please don't open a public issue for a vulnerability. Use the repository's
**Security › Report a vulnerability** tab (private report). You'll get an answer as soon as
possible, and the fix ships in a version published through the releases.

## What Heim protects, and how

| Topic | Protection |
| --- | --- |
| Accounts | Hashed passwords (PBKDF2-SHA256, 100,000 rounds, per-account salt), 8 characters minimum, constant-time comparison. After 10 failures in 5 minutes, sign-ins are refused for a while. |
| Sessions | Random 256-bit token in an `HttpOnly; SameSite=Lax` cookie (`Secure` over HTTPS). The database only keeps its SHA-256 hash: a stolen database opens no session. One year at most. |
| Roles | Every API route requires a role (guest, member, admin); anything not listed is admin-only. A guest controls neither the sirens, nor the alarm, nor automation states. Per-account limits (forced board, editing, read-only devices) are checked by the server. |
| Other sites | A request that changes something, or opens the WebSocket, coming from another page than Heim (even on the same machine, such as Home Assistant) is refused. |
| Browser | Strict content security policy (the only allowed script is the interface's, by its hash), no framing, `nosniff`, no address leaked to other sites, API responses never cached. |
| Secrets | The MQTT password, Tuya local keys, alarm code and notification private key never leave through the API. Internal settings can only be written through their own routes. |
| Notifications | Heim only sends to real push services (Apple, Google, Mozilla, Microsoft); an account only touches the notifications of its own devices. |
| Database | SQLite, a local file never exposed on the network, folder in `700` and files in `600`. Every SQL query uses bound parameters (no injection). SQLite has no Supabase-style “RLS”: access control is done by the API, route by route. |
| System commands | `nmcli`, `ping`, `systemctl`, `hostnamectl`, `timedatectl`, `tailscale` are run with separate arguments, never through a shell. |
| Service | `heim` system user without a shell, `NoNewPrivileges`, read-only file system except its two folders, protected kernel and cgroups, `UMask=0077`, memory limit. Polkit only allows it the network, the machine name, the time zone and shutdown/restart. |
| Updates | Downloaded from GitHub releases over HTTPS, checked by SHA-256, with an automatic rollback if the new version doesn't start. |
| Dependencies | `cargo audit` and `npm audit` on every push and every Monday; Dependabot proposes updates every week. |

## Good to know

- **An account is required** since 0.13: the first-start setup doesn't let you through without
  an administrator account, and a Heim without accounts reopens the setup.
- **Access from outside**: only through Tailscale (HTTPS, no port opened on the router). Never
  open Heim's port to the internet.
- **MQTT**: MQTT has no encryption here, like Home Assistant by default. Heim's built-in broker
  only listens on the box itself (127.0.0.1) unless you open it to the home network; then every
  network device needs an account (generated password). Never forward port 1883 on the router.
- **System files**: Heim only writes a version number (`system.request`); the root service
  `heim-system` downloads that release's `heim-system.tar.gz` from the repository written in
  `/etc/heim/repo` at install time, checks its SHA-256 and applies it. Nothing Heim writes is run
  as root.
- **Zigbee2MQTT on the box** runs as the `heim` user, with its own systemd service. Heim may only
  start, stop and restart that service (polkit). Its web interface is off by default; when on,
  it asks for a token.
- **Backups**: a backup file contains everything, including Tuya keys and the MQTT password.
  Keep it to yourself (never in a public repository).
- **Pi console account** (`heim-admin`): SSH is off. Set the `PI_PASSWORD` secret in the
  repository to replace the images' default password.
