// ui-local.mjs — Logica pura (senza DOM, senza Tauri) del canale shell lato ui:
// quale comando Tauri apre una finestra "locale" chiesta con `open_ui_local`,
// quali finestre Markdown sono singleton (D15), come si legge il window_id di
// una finestra di output dalla sua label. Testabile con node:test.

/** Prefisso delle label delle finestre di output (`open_output_window`). */
export const OUTPUT_LABEL_PREFIX = "output-";

const LOCAL_WINDOWS = {
  config: "open_config_window",
  library: "open_library_window",
  aichat: "open_aichat_window",
};

/**
 * `open_ui_local{name}` → `{cmd, args}` da passare a `invoke`, o `null` se il
 * nome non è né un singleton locale né un canale esterno della tabella.
 * @param {string} name
 * @param {{id:string, windowTitle:string}[]} channels — EXTERNAL_TOOL_CHANNELS
 */
export function resolveUiLocal(name, channels) {
  if (!name) return null;
  if (LOCAL_WINDOWS[name]) return { cmd: LOCAL_WINDOWS[name], args: undefined };
  const ch = channels.find((c) => c.id === name);
  if (ch) return { cmd: "open_external_channel_window", args: { channelId: ch.id, windowTitle: ch.windowTitle } };
  return null;
}

/** Label fissa per le finestre Markdown singleton: solo `/help` (D15). */
export function markdownWindowLabel(kind) {
  return kind === "help" ? "help" : null;
}

/** `output-<id>` → `<id>`; qualunque altra label → null. */
export function outputWindowIdFromLabel(label) {
  if (typeof label !== "string" || !label.startsWith(OUTPUT_LABEL_PREFIX)) return null;
  return label.slice(OUTPUT_LABEL_PREFIX.length);
}
