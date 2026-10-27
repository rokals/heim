//! Comptes, rôles et présence, de bout en bout (serveur HTTP réel, appareils virtuels).

use std::time::Duration;

use heim::{Config, Core};
use serde_json::{Value, json};

fn config(dir: &std::path::Path) -> Config {
    Config {
        data_dir: dir.to_path_buf(),
        http_addr: "127.0.0.1:0".parse().unwrap(),
        token: None,
        demo: true,
        zigbee2mqtt: None,
        broker: false,
    }
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("heim-acc-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

async fn serve(core: Core) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = heim::api::router(core, None, addr.port());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}/api")
}

struct Client {
    base: String,
    http: reqwest::Client,
    cookie: Option<String>,
}

impl Client {
    fn new(base: &str) -> Self {
        Client {
            base: base.into(),
            http: reqwest::Client::new(),
            cookie: None,
        }
    }

    async fn call(&mut self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut req = self
            .http
            .request(method.parse().unwrap(), format!("{}{path}", self.base));
        if let Some(c) = &self.cookie {
            req = req.header("cookie", c);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status().as_u16();
        if let Some(sc) = resp.headers().get("set-cookie") {
            let v = sc.to_str().unwrap().split(';').next().unwrap().to_string();
            self.cookie = (!v.ends_with('=')).then_some(v);
        }
        let v = resp.json::<Value>().await.unwrap_or(Value::Null);
        (status, v)
    }
}

async fn wait_state(core: &Core, id: &str, want: &str) {
    for _ in 0..300 {
        if core
            .registry
            .get(id)
            .await
            .and_then(|e| e.state)
            .is_some_and(|s| s.value == json!(want))
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("{id} n'est pas passé à {want}");
}

#[tokio::test]
async fn roles_protect_the_house() {
    let dir = temp_dir("roles");
    let core = Core::start(&config(&dir)).unwrap();
    wait_state(&core, "light.demo_salon", "off").await;
    let base = serve(core.clone()).await;
    let mut anon = Client::new(&base);

    // Sans compte : ouvert, comme avant.
    assert_eq!(anon.call("GET", "/entities", None).await.0, 200);
    let (_, st) = anon.call("GET", "/auth/status", None).await;
    assert_eq!(st["open"], json!(true));

    // Premier compte : admin, et ce navigateur est connecté.
    let mut admin = Client::new(&base);
    let (code, v) = admin
        .call(
            "POST",
            "/users",
            Some(json!({"name": "Alex", "password": "secret123", "role": "guest"})),
        )
        .await;
    assert_eq!(code, 200, "{v}");
    assert_eq!(v["user"]["role"], json!("admin"));
    assert!(admin.cookie.is_some());

    // Désormais, il faut se connecter.
    let mut anon = Client::new(&base);
    assert_eq!(anon.call("GET", "/entities", None).await.0, 401);
    assert_eq!(anon.call("GET", "/auth/status", None).await.0, 200);
    assert_eq!(admin.call("GET", "/entities", None).await.0, 200);
    let (_, me) = admin.call("GET", "/auth/me", None).await;
    assert_eq!(me["user"]["name"], json!("Alex"));

    // Un invité et un membre.
    for (name, role) in [("Zoé", "guest"), ("Camille", "member")] {
        let (code, v) = admin
            .call(
                "POST",
                "/users",
                Some(json!({"name": name, "password": "motdepasse", "role": role})),
            )
            .await;
        assert_eq!(code, 200, "{v}");
    }
    let mut guest = Client::new(&base);
    assert_eq!(
        guest
            .call(
                "POST",
                "/auth/login",
                Some(json!({"name": "zoé", "password": "faux"}))
            )
            .await
            .0,
        401
    );
    let (code, _) = guest
        .call(
            "POST",
            "/auth/login",
            Some(json!({"name": "Zoé", "password": "motdepasse"})),
        )
        .await;
    assert_eq!(code, 200);
    assert_eq!(guest.call("GET", "/entities", None).await.0, 200);
    let on = json!({"action": "turn_on"});
    assert_eq!(
        guest
            .call(
                "POST",
                "/entities/light.demo_salon/command",
                Some(on.clone())
            )
            .await
            .0,
        202
    );
    assert_eq!(
        guest
            .call(
                "POST",
                "/entities/siren.demo_sirene/command",
                Some(on.clone())
            )
            .await
            .0,
        403
    );
    assert_eq!(
        guest
            .call("POST", "/security/arm", Some(json!({"mode": "away"})))
            .await
            .0,
        403
    );
    assert_eq!(
        guest
            .call(
                "PUT",
                "/settings/location",
                Some(json!({"latitude": 1, "longitude": 1}))
            )
            .await
            .0,
        403
    );
    assert_eq!(guest.call("GET", "/users", None).await.0, 403);

    let mut member = Client::new(&base);
    member
        .call(
            "POST",
            "/auth/login",
            Some(json!({"name": "Camille", "password": "motdepasse"})),
        )
        .await;
    // Un membre peut utiliser l'alarme (ici refusée faute de capteurs, mais pas interdite).
    assert_eq!(
        member
            .call("POST", "/security/arm", Some(json!({"mode": "away"})))
            .await
            .0,
        400
    );
    assert_eq!(
        member
            .call(
                "POST",
                "/users",
                Some(json!({"name": "X", "password": "123456"}))
            )
            .await
            .0,
        403
    );

    // Changer son mot de passe déconnecte les autres appareils.
    let (code, _) = guest
        .call(
            "POST",
            "/auth/password",
            Some(json!({"current": "motdepasse", "new": "nouveau123"})),
        )
        .await;
    assert_eq!(code, 200);
    assert_eq!(guest.call("GET", "/entities", None).await.0, 401);

    // Le dernier admin ne peut pas être rétrogradé.
    assert_eq!(
        admin
            .call("PUT", "/users/alex", Some(json!({"role": "member"})))
            .await
            .0,
        400
    );
    // Déconnexion.
    assert_eq!(admin.call("POST", "/auth/logout", None).await.0, 204);
    assert!(admin.cookie.is_none());
}

#[tokio::test]
async fn everyone_leaving_arms_the_alarm() {
    let dir = temp_dir("presence");
    let core = Core::start(&config(&dir)).unwrap();
    wait_state(&core, "binary_sensor.demo_porte", "off").await;
    core.accounts
        .create("Alex", heim::accounts::Role::Admin, "secret123")
        .unwrap();

    let sec: heim::security::SecurityConfig = serde_json::from_value(json!({
        "sensors": [{"entity_id": "binary_sensor.demo_porte", "role": "perimeter"}],
        "exit_delay_s": 0
    }))
    .unwrap();
    core.settings.set(heim::security::CONFIG_KEY, &sec).unwrap();
    let presence: heim::presence::PresenceConfig = serde_json::from_value(json!({
        "enabled": true,
        "everyone_left": "arm",
        "away_after_s": 1,
        "phones": [{"mac": "DA:A1:19:12:34:56", "user_id": "alex"}]
    }))
    .unwrap();
    core.settings
        .set(heim::presence::CONFIG_KEY, &presence)
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    core.presence.seen("da:a1:19:12:34:56", None).await;
    wait_state(&core, "person.alex", "home").await;
    assert_eq!(
        core.security.status().await.phase,
        heim::security::Phase::Disarmed
    );

    // Plus de nouvelles du téléphone : Alex est parti, la maison est vide.
    tokio::time::sleep(Duration::from_millis(1200)).await;
    core.presence.evaluate().await;
    wait_state(&core, "person.alex", "away").await;
    for _ in 0..100 {
        if core.security.status().await.phase == heim::security::Phase::Armed {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        core.security.status().await.phase,
        heim::security::Phase::Armed
    );

    // Il revient : il est « à la maison », mais l'alarme ne se coupe pas toute seule.
    core.presence.seen("da:a1:19:12:34:56", None).await;
    wait_state(&core, "person.alex", "home").await;
    assert_eq!(
        core.security.status().await.phase,
        heim::security::Phase::Armed
    );
}
