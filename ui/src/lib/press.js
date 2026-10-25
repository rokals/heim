// Appui long sur un élément, au doigt comme à la souris (action Svelte : use:press={…}).
//
// - Un appui court laisse passer le clic normal (allumer la lampe…).
// - Un appui long (450 ms sans bouger) appelle onhold, fait vibrer le téléphone si possible,
//   et avale le clic qui suit : la lampe ne bascule pas en plus.
// - Bouger le doigt de plus de 10 px annule : c'est un défilement, pas un appui.
// - Le menu « copier / partager » de l'iPhone et le clic droit sont bloqués sur l'élément.

const HOLD_MS = 450;
const SLOP = 10;

export function press(node, options) {
  let opts = options || {};
  let timer = null;
  let start = null;
  let held = false;

  function clear() {
    clearTimeout(timer);
    timer = null;
  }

  function swallowNextClick() {
    const stop = (e) => {
      e.stopPropagation();
      e.preventDefault();
    };
    node.addEventListener('click', stop, { capture: true, once: true });
    // Si aucun clic ne suit (le doigt a glissé), on retire le piège.
    setTimeout(() => node.removeEventListener('click', stop, { capture: true }), 600);
  }

  function down(e) {
    if (!opts.onhold || opts.disabled) return;
    if (e.button > 0) return;
    if (e.target.closest('input, select, textarea, [data-nopress]')) return;
    held = false;
    start = { x: e.clientX, y: e.clientY };
    clear();
    timer = setTimeout(() => {
      timer = null;
      held = true;
      try {
        navigator.vibrate?.(12);
      } catch {
        /* pas de vibreur */
      }
      opts.onhold?.(e, start);
    }, opts.ms ?? HOLD_MS);
  }

  function move(e) {
    if (!timer || !start) return;
    if (Math.hypot(e.clientX - start.x, e.clientY - start.y) > SLOP) clear();
  }

  function up() {
    clear();
    if (held) swallowNextClick();
    held = false;
  }

  function menu(e) {
    if (opts.onhold && !opts.disabled) e.preventDefault();
  }

  node.addEventListener('pointerdown', down);
  node.addEventListener('pointermove', move);
  node.addEventListener('pointerup', up);
  node.addEventListener('pointercancel', clear);
  node.addEventListener('pointerleave', clear);
  node.addEventListener('contextmenu', menu);

  return {
    update(next) {
      opts = next || {};
    },
    destroy() {
      clear();
      node.removeEventListener('pointerdown', down);
      node.removeEventListener('pointermove', move);
      node.removeEventListener('pointerup', up);
      node.removeEventListener('pointercancel', clear);
      node.removeEventListener('pointerleave', clear);
      node.removeEventListener('contextmenu', menu);
    },
  };
}
