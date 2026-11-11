// Tiny pub/sub store for app-wide toasts, rendered by <Toaster /> in
// MainLayout. Same shape as libraryStore.js: module state plus subscribe().

const DEFAULT_DURATION_MS = 5000;
const ERROR_DURATION_MS = 8000;

let toasts = []; // [{ id, message, variant: 'info' | 'error', duration }]
let listeners = [];
let nextId = 1;
const timers = new Map();

function emit() {
  listeners.forEach((l) => l(toasts));
}

export function getToasts() {
  return toasts;
}

export function subscribe(listener) {
  listeners.push(listener);
  return () => {
    listeners = listeners.filter((l) => l !== listener);
  };
}

export function dismissToast(id) {
  clearTimeout(timers.get(id));
  timers.delete(id);
  const next = toasts.filter((t) => t.id !== id);
  if (next.length === toasts.length) return;
  toasts = next;
  emit();
}

/**
 * Show a toast. Passing an `id` that is already shown replaces that toast
 * (and restarts its timer) instead of stacking a duplicate.
 * @param {string} message
 * @param {Object} [options]
 * @param {'info'|'error'} [options.variant='info']
 * @param {number|null} [options.duration] ms before auto-dismiss; null keeps
 *   it until the user dismisses it
 * @param {string|number} [options.id]
 * @returns {string|number} the toast id, for dismissToast()
 */
export function showToast(message, { variant = "info", duration, id } = {}) {
  const toastId = id ?? `toast-${nextId++}`;
  const ms =
    duration === undefined
      ? variant === "error"
        ? ERROR_DURATION_MS
        : DEFAULT_DURATION_MS
      : duration;

  clearTimeout(timers.get(toastId));
  timers.delete(toastId);
  const toast = { id: toastId, message, variant, duration: ms };
  const i = toasts.findIndex((t) => t.id === toastId);
  toasts =
    i === -1
      ? [...toasts, toast]
      : toasts.map((t) => (t.id === toastId ? toast : t));
  if (ms !== null) {
    timers.set(
      toastId,
      setTimeout(() => dismissToast(toastId), ms),
    );
  }
  emit();
  return toastId;
}

/**
 * The text to show for a rejected invoke() or a thrown Error. Tauri commands
 * reject with the backend's `Failed to …` string itself.
 * @param {unknown} err
 * @returns {string}
 */
export function errorMessage(err) {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  if (err && typeof err.message === "string") return err.message;
  return String(err);
}

/**
 * Show an error toast. `err` may be a string, an Error, or a rejected
 * invoke() value; `context` (e.g. "Couldn't play file") is prefixed.
 * @param {unknown} err
 * @param {Object} [options] showToast options plus `context`
 * @returns {string|number}
 */
export function showError(err, { context, ...options } = {}) {
  const text = errorMessage(err);
  return showToast(context ? `${context}: ${text}` : text, {
    variant: "error",
    ...options,
  });
}
