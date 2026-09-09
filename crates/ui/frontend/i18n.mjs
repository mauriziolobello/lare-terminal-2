// i18n.mjs — Frontend internationalization module for Lare Terminal.
//
// Exposes:
// - `t(key, params)`: translates literal key with optional {name} interpolation.
//   Fallback: if key is missing, returns the key itself.
// - `initI18n(dict)`: sets dictionary in memory.
// - `getDict()`: gets copy of in-memory dictionary.
// - `fetchI18n(invoke, lang)`: requests dictionary from Tauri IPC `get_i18n`.
// - `applyI18n(root)`: walks DOM and updates data-i18n, data-i18n-placeholder,
//   data-i18n-title, and data-i18n-aria-label attributes.

let currentDict = {};

/**
 * Initializes the in-memory dictionary.
 * @param {Record<string, string>} dict
 */
export function initI18n(dict) {
  currentDict = dict && typeof dict === "object" ? { ...dict } : {};
}

/**
 * Returns a copy of the current in-memory dictionary.
 * @returns {Record<string, string>}
 */
export function getDict() {
  return { ...currentDict };
}

/**
 * Translates a key with optional interpolation.
 * ONLY literal keys should be passed to t() in application code.
 *
 * @param {string} key
 * @param {Record<string, any>} [params]
 * @returns {string}
 */
export function t(key, params) {
  if (typeof key !== "string") return "";
  let text = currentDict[key] ?? key;
  if (params && typeof params === "object") {
    for (const [pKey, pVal] of Object.entries(params)) {
      text = text.replaceAll(`{${pKey}}`, String(pVal));
    }
  }
  return text;
}

/**
 * Fetches dictionary via Tauri IPC and initializes in-memory state.
 * Infallible to callers (catches errors and preserves existing state).
 *
 * @param {Function} invoke - Tauri invoke function
 * @param {string} [lang] - Language code (default "it")
 * @returns {Promise<Record<string, string>>}
 */
export async function fetchI18n(invoke, lang = "it") {
  if (!invoke || typeof invoke !== "function") {
    return currentDict;
  }
  try {
    const dict = await invoke("get_i18n", { lang: lang || "it" });
    initI18n(dict);
  } catch (err) {
    console.warn("[i18n] fetchI18n failed:", err);
  }
  return currentDict;
}

/**
 * Walks a DOM container and applies translations to elements with data-i18n attributes.
 *
 * Supported attributes:
 * - `data-i18n`: sets textContent
 * - `data-i18n-placeholder`: sets placeholder attribute
 * - `data-i18n-title`: sets title attribute
 * - `data-i18n-aria-label`: sets aria-label attribute
 *
 * @param {ParentNode} [root] - Container to search (defaults to document if available)
 */
export function applyI18n(root) {
  const container = root || (typeof document !== "undefined" ? document : null);
  if (!container || typeof container.querySelectorAll !== "function") return;

  // textContent
  for (const el of container.querySelectorAll("[data-i18n]")) {
    const key = el.getAttribute("data-i18n");
    if (key) el.textContent = t(key);
  }

  // placeholder
  for (const el of container.querySelectorAll("[data-i18n-placeholder]")) {
    const key = el.getAttribute("data-i18n-placeholder");
    if (key) el.setAttribute("placeholder", t(key));
  }

  // title
  for (const el of container.querySelectorAll("[data-i18n-title]")) {
    const key = el.getAttribute("data-i18n-title");
    if (key) el.setAttribute("title", t(key));
  }

  // aria-label
  for (const el of container.querySelectorAll("[data-i18n-aria-label]")) {
    const key = el.getAttribute("data-i18n-aria-label");
    if (key) el.setAttribute("aria-label", t(key));
  }
}
