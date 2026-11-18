// Service worker de Heim : affiche les notifications push, même application fermée, et
// garde une copie de l'application pour qu'elle s'ouvre tout de suite, même quand le Wi-Fi
// tarde : elle affiche alors « Connexion à Heim… » et se reconnecte toute seule, au lieu
// d'un écran noir puis blanc. Les données, elles, viennent toujours de la box.

const SHELL = 'heim-shell-v1';
const FILES = ['/', '/manifest.webmanifest', '/icon-192.png', '/apple-touch-icon.png'];
// Au-delà, on ouvre la copie gardée (la page la plus récente sera prise la fois suivante).
const WAIT_MS = 3500;

self.addEventListener('install', (e) => {
  e.waitUntil(
    caches
      .open(SHELL)
      .then((c) => c.addAll(FILES))
      .catch(() => {})
      .then(() => self.skipWaiting()),
  );
});

self.addEventListener('activate', (e) => {
  e.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((k) => k.startsWith('heim-shell') && k !== SHELL).map((k) => caches.delete(k))))
      .then(() => self.clients.claim()),
  );
});

self.addEventListener('fetch', (event) => {
  const req = event.request;
  if (req.method !== 'GET') return;
  const url = new URL(req.url);
  if (url.origin !== self.location.origin || url.pathname.startsWith('/api')) return;
  if (req.mode === 'navigate') {
    event.respondWith(page(req));
  } else if (FILES.includes(url.pathname)) {
    event.respondWith(caches.match(url.pathname).then((hit) => hit || fetch(req)));
  }
});

/** La page : la box d'abord ; si elle tarde ou ne répond pas, la copie gardée. */
async function page(req) {
  const cache = await caches.open(SHELL);
  const network = fetch(req).then((res) => {
    if (res.ok) cache.put('/', res.clone()).catch(() => {});
    return res;
  });
  const saved = await cache.match('/');
  if (!saved) return network;
  const late = new Promise((resolve) => setTimeout(() => resolve(saved), WAIT_MS));
  return Promise.race([network.catch(() => saved), late]);
}

self.addEventListener('push', (event) => {
  let data = {};
  try {
    data = event.data ? event.data.json() : {};
  } catch {
    data = { message: event.data ? event.data.text() : '' };
  }
  const urgent = data.level === 'urgent';
  event.waitUntil(
    self.registration.showNotification(data.title || 'Heim', {
      body: data.message || '',
      tag: data.tag || undefined,
      renotify: Boolean(data.tag),
      requireInteraction: urgent,
      icon: '/icon-192.png',
      badge: '/icon-192.png',
      data: { url: data.url || '/' },
    }),
  );
});

self.addEventListener('notificationclick', (event) => {
  event.notification.close();
  const url = new URL(event.notification.data?.url || '/', self.location.origin).href;
  event.waitUntil(
    self.clients.matchAll({ type: 'window', includeUncontrolled: true }).then((list) => {
      for (const client of list) {
        if (client.url.startsWith(self.location.origin) && 'focus' in client) {
          if ('navigate' in client) client.navigate(url).catch(() => {});
          return client.focus();
        }
      }
      return self.clients.openWindow(url);
    }),
  );
});
