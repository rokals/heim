// Notifications push : ce que le navigateur sait faire, et l'abonnement.
import { api } from './store.svelte.js';

/** Où en est ce navigateur ? Sert à expliquer à l'utilisateur ce qu'il lui manque. */
export function pushSupport() {
  const ua = navigator.userAgent;
  const ios = /iPhone|iPad|iPod/.test(ua) || (/Macintosh/.test(ua) && navigator.maxTouchPoints > 1);
  const standalone = matchMedia('(display-mode: standalone)').matches || navigator.standalone === true;
  return {
    ios,
    standalone,
    secure: window.isSecureContext,
    available: 'serviceWorker' in navigator && 'PushManager' in window && 'Notification' in window,
    permission: 'Notification' in window ? Notification.permission : 'unsupported',
  };
}

/** Ce qu'il faut faire avant de pouvoir activer, ou `null` si tout est prêt. */
export function pushBlocker(s = pushSupport()) {
  if (!s.secure) return 'https';
  if (s.ios && !s.standalone) return 'install';
  if (!s.available) return 'unsupported';
  if (s.permission === 'denied') return 'denied';
  return null;
}

function keyBytes(b64) {
  const pad = '='.repeat((4 - (b64.length % 4)) % 4);
  const raw = atob((b64 + pad).replace(/-/g, '+').replace(/_/g, '/'));
  return Uint8Array.from(raw, (c) => c.charCodeAt(0));
}

export async function registerWorker() {
  if (!('serviceWorker' in navigator) || !window.isSecureContext) return null;
  try {
    return await navigator.serviceWorker.register('/sw.js');
  } catch {
    return null;
  }
}

export async function currentSubscription() {
  if (!('serviceWorker' in navigator) || !window.isSecureContext) return null;
  const reg = await navigator.serviceWorker.getRegistration();
  return reg ? reg.pushManager.getSubscription() : null;
}

/**
 * À appeler directement dans le clic : iOS refuse de demander l'autorisation autrement.
 */
export async function enablePush(targetId) {
  const permission = await Notification.requestPermission();
  if (permission !== 'granted') throw new Error("Les notifications ont été refusées. Vous pouvez les autoriser dans les réglages du téléphone.");
  const reg = (await registerWorker()) ?? (await navigator.serviceWorker.ready);
  await navigator.serviceWorker.ready;
  const { public_key } = await api('/push/key');
  let sub = await reg.pushManager.getSubscription();
  if (!sub) {
    sub = await reg.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: keyBytes(public_key) });
  }
  await api('/push/subscribe', { method: 'POST', body: { target_id: targetId, subscription: sub.toJSON() } });
  return sub;
}

export async function disablePush(targetId) {
  const sub = await currentSubscription();
  if (sub) await sub.unsubscribe().catch(() => {});
  await api('/push/subscribe', { method: 'DELETE', body: { target_id: targetId } });
}

export async function testPush(targetId) {
  await api('/push/test', { method: 'POST', body: { target_id: targetId } });
}
