// config-dialog.js — /config in-overlay dialog for Lare Terminal.
//
// Responsibility (SRP): owns the config dialog lifecycle — build DOM,
// populate fields, handle Save/Cancel/Esc, invoke Tauri commands.
// Does NOT own the main overlay, the WebSocket, or the renderer.
//
// The dialog is an in-overlay <div> panel (not a second Tauri window).
// It reads current config via `get_config`, saves via `set_config`, and
// calls back the parent (app.js) on close so it can re-apply CSS vars.
//
// Usage:
//   import { ConfigDialog } from "./config-dialog.js";
//   const dlg = new ConfigDialog(containerEl, tauriInvoke, onConfigSaved, onClose);
//   dlg.open();
// `onClose` (opzionale) viene invocato in close(): la finestra dedicata /config
// lo usa per chiudere la propria webview (Cancel/Esc/dopo-save).

import { LareWsClient } from "./ws-client.js";
import { isConnectionFailureStatus } from "./expand-prompt.mjs";

// ---------------------------------------------------------------------------
// Key-capture helpers
// ---------------------------------------------------------------------------

/**
 * Convert a KeyboardEvent to a Tauri/global-hotkey accelerator string.
 *
 * Rules:
 *   - Modifier-only events (Shift, Ctrl, Alt, Meta alone) are ignored.
 *   - Modifiers appear in order: Ctrl+ / Alt+ / Shift+ / Super+
 *   - Then the key: we use `e.code` for function keys and translate to
 *     the format the parser expects (F1..F12 → "F1".."F12"; other keys
 *     → normalised label).
 *
 * @param {KeyboardEvent} e
 * @returns {string|null}  accelerator string or null if the event is
 *                         modifier-only or otherwise unmappable.
 */
function keyEventToAccelerator(e) {
  const MODIFIER_KEYS = new Set([
    "Control", "Shift", "Alt", "Meta",
    "ControlLeft", "ControlRight",
    "ShiftLeft", "ShiftRight",
    "AltLeft", "AltRight",
    "MetaLeft", "MetaRight",
  ]);

  // Ignore stand-alone modifier key presses.
  if (MODIFIER_KEYS.has(e.key) || MODIFIER_KEYS.has(e.code)) return null;

  const parts = [];
  if (e.ctrlKey)  parts.push("Ctrl");
  if (e.altKey)   parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  if (e.metaKey)  parts.push("Super");

  // Map e.code / e.key to the accelerator token.
  const token = codeToToken(e.code, e.key);
  if (!token) return null;

  parts.push(token);
  return parts.join("+");
}

/**
 * Map a DOM `code` (physical key) and `key` (logical character) to the
 * token expected by Tauri's global-hotkey accelerator parser.
 *
 * @param {string} code  e.code
 * @param {string} key   e.key
 * @returns {string|null}
 */
function codeToToken(code, key) {
  // Function keys F1–F12.
  const fMatch = code.match(/^F(\d{1,2})$/);
  if (fMatch) return `F${fMatch[1]}`;

  // Letter keys: use the uppercase letter.
  const letterMatch = code.match(/^Key([A-Z])$/);
  if (letterMatch) return letterMatch[1];

  // Digit keys.
  const digitMatch = code.match(/^Digit(\d)$/);
  if (digitMatch) return digitMatch[1];

  // Common named keys the parser understands.
  const NAMED = {
    Space:       "Space",
    Enter:       "Return",
    Tab:         "Tab",
    Backspace:   "Backspace",
    Delete:      "Delete",
    Insert:      "Insert",
    Home:        "Home",
    End:         "End",
    PageUp:      "PageUp",
    PageDown:    "PageDown",
    ArrowLeft:   "Left",
    ArrowRight:  "Right",
    ArrowUp:     "Up",
    ArrowDown:   "Down",
    Escape:      "Escape",
  };
  if (NAMED[code]) return NAMED[code];

  // Numpad keys.
  const numpadMatch = code.match(/^Numpad(\d)$/);
  if (numpadMatch) return `Numpad${numpadMatch[1]}`;

  // Fallback: use the key label if it's a single printable character.
  if (key && key.length === 1) return key.toUpperCase();

  return null;
}

// ---------------------------------------------------------------------------
// ConfigDialog class
// ---------------------------------------------------------------------------

export class ConfigDialog {
  /**
   * @param {HTMLElement}  containerEl   The overlay panel to attach the dialog div to.
   * @param {Function}     tauriInvoke   `window.__TAURI__.core.invoke` (or null in tests).
   * @param {Function}     onSaved       Called with the saved Config object after a
   *                                     successful save, so the caller can apply CSS vars.
   */
  constructor(containerEl, tauriInvoke, onSaved, onClose) {
    this._container  = containerEl;
    this._invoke     = tauriInvoke;
    this._onSaved    = onSaved || (() => {});
    // onClose: chiamato in close() — usato dalla finestra dedicata /config per
    // chiudersi (Cancel/Esc/dopo-save). Default no-op per i chiamatori a 3 arg.
    this._onClose    = onClose || (() => {});

    /** @type {HTMLElement|null} */
    this._panel = null;

    // Bind Esc handler so we can remove it on close.
    this._escHandler = (e) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        this.close();
      }
    };

    // Bind key-capture handler; assigned to the capture input when open.
    this._captureHandler = null;
  }

  // ── Public API ────────────────────────────────────────────────────────────

  /** Open the /config dialog: load current config then show the panel. */
  async open() {
    if (this._panel) return; // already open

    let current = {
      action_key:          "F2",
      cursor_color:        "#FFFFFF",
      cursor_font:         "Consolas",
      cursor_size:         11,
      position:            "center",
      activity_indicator:  "title",
      web_search_enabled:  true,
    };

    if (this._invoke) {
      try {
        current = await this._invoke("get_config");
      } catch (e) {
        console.error("[config-dialog] get_config error:", e);
      }
    }

    let search = { result_cap: 2000, max_depth: 8 };
    if (this._invoke) {
      try { search = await this._invoke("get_search_settings"); }
      catch (e) { console.error("[config-dialog] get_search_settings error:", e); }
    }

    let aichat = {
      enabled: false, label_base: "lare", chat_port: 40100,
      ai_participates: true, ai_autoparticipate: false,
      display_name: null, ai_display_name: null,
    };
    if (this._invoke) {
      try { aichat = await this._invoke("get_aichat_settings"); }
      catch (e) { console.error("[config-dialog] get_aichat_settings error:", e); }
    }

    let llm = { active: "", providers: [] };
    if (this._invoke) {
      try { llm = await this._invoke("get_llm_settings"); }
      catch (e) { console.error("[config-dialog] get_llm_settings error:", e); }
    }

    let marketData = { active: "yfinance", sources: [{ kind: "yfinance" }] };
    if (this._invoke) {
      try { marketData = await this._invoke("get_market_data_settings"); }
      catch (e) { console.error("[config-dialog] get_market_data_settings error:", e); }
    }

    this._buildPanel(current, search, aichat, llm, marketData);
    window.addEventListener("keydown", this._escHandler, { capture: true });
  }

  /** Close the dialog (Cancel path — no save). */
  close() {
    if (!this._panel) return;
    this._panel.remove();
    this._panel = null;
    window.removeEventListener("keydown", this._escHandler, { capture: true });
    this._onClose();   // notifica il proprietario (finestra /config → close_self)
  }

  /** Whether the dialog is currently open. */
  isOpen() {
    return this._panel !== null;
  }

  // ── Private: DOM construction ─────────────────────────────────────────────

  /**
   * Build and attach the modal panel to the container.
   * Uses DOM methods throughout — no innerHTML with external data.
   *
   * @param {object} cfg        Current UI config from Tauri (or defaults).
   * @param {object} search     Current search settings from Tauri (or defaults).
   * @param {object} aichat     Current AI Chat settings from Tauri (or defaults).
   * @param {object} llm        Current LLM provider settings from Tauri (or defaults).
   * @param {object} marketData Current market data settings from Tauri (or defaults).
   */
  _buildPanel(cfg, search, aichat, llm, marketData) {
    // Overlay backdrop that dims the main content.
    const panel = document.createElement("div");
    panel.className = "config-dialog-overlay";
    this._panel = panel;

    // Dialog box.
    const box = document.createElement("div");
    box.className = "config-dialog-box";
    panel.appendChild(box);

    // ── Title ──
    const title = document.createElement("h2");
    title.className = "config-dialog-title";
    title.textContent = "Configuration — /config";
    box.appendChild(title);

    // ── Tab bar ──
    const tabsBar = document.createElement("div");
    tabsBar.className = "config-tabs";
    box.appendChild(tabsBar);

    // ── Tab panels container ──
    const panelsWrap = document.createElement("div");
    panelsWrap.className = "config-tab-panels";
    box.appendChild(panelsWrap);

    // Pannello UI: contiene i campi di configurazione visuale.
    const uiPanel = document.createElement("div");
    uiPanel.className = "config-tab-panel";
    panelsWrap.appendChild(uiPanel);
    const uiRefs = this._buildUiTab(uiPanel, cfg);

    // Pannello Search: contiene result_cap e max_depth.
    const searchPanel = document.createElement("div");
    searchPanel.className = "config-tab-panel hidden";
    panelsWrap.appendChild(searchPanel);
    const searchRefs = this._buildSearchTab(searchPanel, search);

    // Pannello AI Chat: contiene enabled/label_base/chat_port di network.json.
    const aichatPanel = document.createElement("div");
    aichatPanel.className = "config-tab-panel hidden";
    panelsWrap.appendChild(aichatPanel);
    const aichatRefs = this._buildAiChatTab(aichatPanel, aichat);

    // Pannello LLM: sola selezione del provider attivo in llms.json.
    const llmPanel = document.createElement("div");
    llmPanel.className = "config-tab-panel hidden";
    panelsWrap.appendChild(llmPanel);
    const llmRefs = this._buildLlmTab(llmPanel, llm);

    // Pannello Fonte Dati Mercato: sola selezione della fonte attiva in market_data.json.
    const marketDataPanel = document.createElement("div");
    marketDataPanel.className = "config-tab-panel hidden";
    panelsWrap.appendChild(marketDataPanel);
    const marketDataRefs = this._buildMarketDataTab(marketDataPanel, marketData);

    // Tab descriptor list (extensible: add future tabs here).
    const TABS = [
      { id: "ui",     label: "UI",      panel: uiPanel },
      { id: "search", label: "Search",  panel: searchPanel },
      { id: "aichat", label: "AI Chat", panel: aichatPanel },
      { id: "llm",    label: "LLM",     panel: llmPanel },
      { id: "market-data", label: "Dati Mercato", panel: marketDataPanel },
    ];

    const tabBtns = [];
    const showTab = (id) => {
      for (const t of TABS) t.panel.classList.toggle("hidden", t.id !== id);
      for (const b of tabBtns) b.classList.toggle("active", b.dataset.tab === id);
    };
    for (const t of TABS) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "config-tab-btn";
      btn.dataset.tab = t.id;
      btn.textContent = t.label;
      btn.addEventListener("click", () => showTab(t.id));
      tabsBar.appendChild(btn);
      tabBtns.push(btn);
    }
    showTab("ui"); // UI tab visible by default

    // ── Error display ──
    const errorEl = document.createElement("p");
    errorEl.className = "config-dialog-error";
    errorEl.setAttribute("role", "alert");
    box.appendChild(errorEl);

    // ── Buttons ──
    const buttons = document.createElement("div");
    buttons.className = "config-dialog-buttons";
    box.appendChild(buttons);

    const saveBtn = document.createElement("button");
    saveBtn.className = "config-btn config-btn-primary";
    saveBtn.textContent = "Salva";
    buttons.appendChild(saveBtn);

    const cancelBtn = document.createElement("button");
    cancelBtn.className = "config-btn config-btn-secondary";
    cancelBtn.textContent = "Annulla";
    buttons.appendChild(cancelBtn);

    // ── Event wiring ──
    cancelBtn.addEventListener("click", () => this.close());

    saveBtn.addEventListener("click", async () => {
      errorEl.textContent = "";

      const sizeVal = parseInt(uiRefs.sizeInput.value, 10);
      if (!Number.isFinite(sizeVal) || sizeVal < 1) {
        errorEl.textContent = "Size must be a positive integer.";
        return;
      }
      const idleDuck = parseInt(uiRefs.idleDuckInput.value, 10);
      if (!Number.isFinite(idleDuck) || idleDuck < 0) {
        errorEl.textContent = "Duck idle deve essere un intero ≥ 0 (0 = disabilitato).";
        return;
      }
      const resultCap = parseInt(searchRefs.resultCapInput.value, 10);
      if (!Number.isFinite(resultCap) || resultCap < 1) {
        errorEl.textContent = "Risultati max deve essere un intero positivo.";
        return;
      }
      const maxDepth = parseInt(searchRefs.maxDepthInput.value, 10);
      if (!Number.isFinite(maxDepth) || maxDepth < 1) {
        errorEl.textContent = "Profondità max deve essere un intero positivo.";
        return;
      }
      const aichatLabel = aichatRefs.labelInput.value.trim();
      if (aichatLabel.length === 0) {
        errorEl.textContent = "AI Chat — Etichetta: non può essere vuota.";
        return;
      }
      const aichatPort = parseInt(aichatRefs.portInput.value, 10);
      if (!Number.isFinite(aichatPort) || aichatPort < 1024 || aichatPort > 65535) {
        errorEl.textContent = "AI Chat — Porta: deve essere un intero tra 1024 e 65535.";
        return;
      }

      const newCfg = {
        action_key:          uiRefs.actionKeyInput.dataset.accelerator || cfg.action_key,
        cursor_color:        uiRefs.colorField.getValue(),
        cursor_font:         uiRefs.fontSelect.value,
        cursor_size:         sizeVal,
        position:            uiRefs.posSelect.value,
        activity_indicator:  uiRefs.activitySelect.value,
        web_search_enabled:  uiRefs.webSearchInput.checked,
        idle_duck_minutes:   idleDuck,
        window_alpha:        uiRefs.alphaField.getValue() / 100,
      };
      const newSearch = { result_cap: resultCap, max_depth: maxDepth };
      const newAiChat = {
        enabled:            aichatRefs.enabledInput.checked,
        label_base:         aichatLabel,
        chat_port:          aichatPort,
        ai_participates:    aichatRefs.participatesInput.checked,
        ai_autoparticipate: aichatRefs.autoparticipateInput.checked,
        display_name:       aichatRefs.displayNameInput.value.trim() || null,
        ai_display_name:    aichatRefs.aiDisplayNameInput.value.trim() || null,
      };

      if (!this._invoke) {
        // No Tauri (running outside overlay, e.g. browser dev).
        console.warn("[config-dialog] No Tauri invoke; not saving.");
        this._onSaved(newCfg);
        this.close();
        return;
      }

      try {
        await this._invoke("set_config", { newCfg });
        await this._invoke("set_search_settings", { settings: newSearch });
        await this._invoke("set_aichat_settings", { settings: newAiChat });
        if (llmRefs.radios.length > 0) {
          const chosen = llmRefs.radios.find((r) => r.checked)?.value;
          if (chosen) await this._invoke("set_llm_settings", { active: chosen });
        }
        if (marketDataRefs.radios.length > 0) {
          const chosenSource = marketDataRefs.radios.find((r) => r.checked)?.value;
          if (chosenSource) await this._invoke("set_market_data_settings", { active: chosenSource });
        }
        this._onSaved(newCfg);
        this.close();
      } catch (err) {
        // set_config / set_search_settings / set_aichat_settings / set_llm_settings / set_market_data_settings
        // return an Err string for invalid values.
        errorEl.textContent = typeof err === "string" ? err : String(err);
      }
    });

    // Attach to container.
    this._container.appendChild(panel);
  }

  /**
   * Build the UI tab content: all visual-configuration fields.
   * Extracted from _buildPanel to keep it focused on layout/wiring.
   *
   * @param {HTMLElement} panel  The tab panel element to append into.
   * @param {object}      cfg    Current UI config.
   * @returns {{ actionKeyInput, colorField, fontSelect, sizeInput, posSelect, activitySelect, webSearchInput }}
   */
  _buildUiTab(panel, cfg) {
    const fields = document.createElement("div");
    fields.className = "config-dialog-fields";
    panel.appendChild(fields);

    // Action key (capture mode).
    const actionKeyInput = this._buildActionKeyField(fields, cfg.action_key);

    // Cursor color — swatch grid + native picker.
    const colorField = this._buildColorField(fields, cfg.cursor_color);

    // Cursor font — dropdown of installed monospace fonts.
    const fontSelect = this._buildFontSelect(fields, cfg.cursor_font);

    // Cursor size.
    const sizeInput = this._buildField(
      fields, "Size (px)", "size", "config-size",
      "number", String(cfg.cursor_size)
    );
    sizeInput.min = "6";
    sizeInput.max = "72";

    // Position.
    const posSelect = this._buildPositionSelect(fields, cfg.position);

    // Activity indicator style.
    const activitySelect = this._buildActivitySelect(fields, cfg.activity_indicator);

    // Ricerca web interna (toggle).
    const webSearchInput = this._buildCheckbox(
      fields, "Ricerca web", "config-web-search", cfg.web_search_enabled
    );

    // Timeout pulcino idle (0 = disabilitato).
    const idleDuckInput = this._buildField(
      fields, "Duck idle (min, 0=off)", "idle-duck", "config-idle-duck",
      "number", String(cfg.idle_duck_minutes ?? 5)
    );
    idleDuckInput.min = "0";
    idleDuckInput.max = "60";

    // Trasparenza (alpha) — slider 0-100%, salvato come window_alpha (0.0-1.0).
    const alphaField = this._buildSliderField(
      fields, "Trasparenza", "window-alpha", "config-window-alpha",
      0, 100, 1, Math.round((cfg.window_alpha ?? 0.87) * 100)
    );

    return { actionKeyInput, colorField, fontSelect, sizeInput, posSelect, activitySelect, webSearchInput, idleDuckInput, alphaField };
  }

  /**
   * Build the Search tab content: result_cap and max_depth fields.
   *
   * @param {HTMLElement} panel   The tab panel element to append into.
   * @param {object}      search  Current search settings { result_cap, max_depth }.
   * @returns {{ resultCapInput, maxDepthInput }}
   */
  _buildSearchTab(panel, search) {
    const fields = document.createElement("div");
    fields.className = "config-dialog-fields";
    panel.appendChild(fields);

    // Maximum number of results returned by /find.
    const resultCapInput = this._buildField(
      fields, "Risultati max", "result-cap", "config-result-cap",
      "number", String(search.result_cap ?? 2000)
    );
    resultCapInput.min = "1";
    resultCapInput.max = "100000";

    // Maximum directory depth explored by /find.
    const maxDepthInput = this._buildField(
      fields, "Profondità max", "max-depth", "config-max-depth",
      "number", String(search.max_depth ?? 8)
    );
    maxDepthInput.min = "1";
    maxDepthInput.max = "64";

    return { resultCapInput, maxDepthInput };
  }

  /**
   * Build the AI Chat tab content: enabled/label_base/chat_port fields
   * (mirror di network.json — già `aichat.json`, rinominato dal piano "Blocco
   * note"; vedi `crates/ui/src-tauri/src/aichat_settings.rs`).
   *
   * A differenza del tab Search, queste impostazioni NON sono "live": il
   * servizio AI Chat dell'orchestrator legge network.json solo al proprio
   * avvio — la nota in fondo al tab lo segnala esplicitamente.
   *
   * @param {HTMLElement} panel   The tab panel element to append into.
   * @param {object}      aichat  Current AI Chat settings { enabled, label_base, chat_port, ai_participates, ai_autoparticipate, display_name, ai_display_name }.
   * @returns {{ enabledInput, labelInput, displayNameInput, aiDisplayNameInput, portInput, participatesInput, autoparticipateInput }}
   */
  _buildAiChatTab(panel, aichat) {
    const fields = document.createElement("div");
    fields.className = "config-dialog-fields";
    panel.appendChild(fields);

    // Attiva/disattiva il servizio AI Chat.
    const enabledInput = this._buildCheckbox(
      fields, "Attivo", "config-aichat-enabled", aichat.enabled
    );

    // Etichetta base di questa macchina (diventa "<label_base>-human" sul wire).
    const labelInput = this._buildField(
      fields, "Etichetta", "aichat-label", "config-aichat-label",
      "text", aichat.label_base ?? "lare"
    );

    // Nickname dell'umano — mostrato in AI Chat al posto di "<label_base>-human".
    // Testo libero (spazi/accenti ok), a differenza di "Etichetta" sopra.
    const displayNameInput = this._buildField(
      fields, "Il tuo nome", "aichat-display-name", "config-aichat-display-name",
      "text", aichat.display_name ?? ""
    );

    // Nickname dell'AI — normalmente scelto dall'AI stessa la prima volta che
    // AI Chat è attivo (chiede all'utente o si sceglie un nome da sola);
    // resta comunque modificabile a mano qui in ogni momento.
    const aiDisplayNameInput = this._buildField(
      fields, "Nome dell'AI", "aichat-ai-display-name", "config-aichat-ai-display-name",
      "text", aichat.ai_display_name ?? ""
    );

    // Porta TCP/UDP della chat.
    const portInput = this._buildField(
      fields, "Porta", "aichat-port", "config-aichat-port",
      "number", String(aichat.chat_port ?? 40100)
    );
    portInput.min = "1024";
    portInput.max = "65535";

    // AI Chat Slice 1b: flag "la mia AI partecipa" — gata SOLO le invocazioni
    // REMOTE (@all / @<mio-label>-ai da un umano di un'altra macchina); la
    // propria invocazione (@ai nella propria finestra) agisce sempre,
    // indipendentemente da questo flag. Default checked (vedi
    // AiChatConfig::default() lato orchestrator, design §9.4).
    const participatesInput = this._buildCheckbox(
      fields, "La mia AI partecipa (risponde alle richieste della stanza)",
      "config-aichat-participates", aichat.ai_participates ?? true
    );

    // AI Chat Slice 2: flag "auto-partecipazione" — a differenza del checkbox
    // sopra (risponde a un'invocazione ESPLICITA, @all/@<mio-label>-ai), questo fa
    // intervenire l'AI SPONTANEAMENTE sui messaggi normali della stanza, giudicando
    // da sola la rilevanza. Più aggressivo/costoso (rimuove la loop-safety-per-
    // costruzione delle Slice 1a/1b, bounded solo dal cap sui turni consecutivi
    // lato orchestrator): opt-in esplicito, default UNCHECKED (a differenza del
    // checkbox sopra, che è default checked).
    const autoparticipateInput = this._buildCheckbox(
      fields, "Auto-partecipazione (l'AI interviene da sola)",
      "config-aichat-autoparticipate", aichat.ai_autoparticipate ?? false
    );

    // Nota: queste impostazioni non sono live, a differenza di Search.
    const note = document.createElement("p");
    note.className = "config-note";
    note.textContent = "Le modifiche all'AI Chat richiedono il riavvio dell'orchestrator.";
    panel.appendChild(note);

    return {
      enabledInput, labelInput, displayNameInput, aiDisplayNameInput,
      portInput, participatesInput, autoparticipateInput,
    };
  }

  /**
   * Build the LLM provider tab content: a radio list of providers already
   * present in llms.json, plus a "Salva" of whichever is selected — SOLA
   * SELEZIONE, non aggiunge/rimuove provider né tocca le API key (quello
   * resta editing a mano del file JSON, vedi
   * `crates/ui/src-tauri/src/llm_settings.rs`).
   *
   * @param {HTMLElement} panel  The tab panel element to append into.
   * @param {object}      llm    Current LLM settings { active, providers: [{name, model}] }.
   * @returns {{ radios: HTMLInputElement[] }}
   */
  _buildLlmTab(panel, llm) {
    const fields = document.createElement("div");
    fields.className = "config-dialog-fields";
    panel.appendChild(fields);

    const providers = llm.providers || [];
    if (providers.length === 0) {
      const note = document.createElement("p");
      note.className = "config-note";
      note.textContent =
        "Nessun llms.json configurato. Crealo a mano — normalmente in " +
        "%LOCALAPPDATA%\\dev.lare.terminal\\llms.json, oppure nella cartella di LARE_LOCAL_DIR " +
        "se impostata — per scegliere un provider diverso da Claude diretto.";
      panel.appendChild(note);
      return { radios: [] };
    }

    const radios = providers.map((p, i) =>
      this._buildRadio(
        fields, `${p.name} — ${p.model}`, "config-llm-active",
        `config-llm-provider-${i}`, p.name, p.name === llm.active
      )
    );

    const note = document.createElement("p");
    note.className = "config-note";
    note.textContent = "Le modifiche al provider LLM richiedono il riavvio dell'orchestrator.";
    panel.appendChild(note);

    return { radios };
  }

  /**
   * Build the Market Data source tab content: a radio list of sources
   * present in market_data.json, plus a "Salva" of whichever is selected —
   * SOLA SELEZIONE, mirror esatto di `_buildLlmTab`. Port/client_id restano
   * editing a mano del file JSON (vedi `market_data_settings.rs`).
   *
   * @param {HTMLElement} panel       The tab panel element to append into.
   * @param {object}      marketData  Current settings { active, sources: [{kind}] }.
   * @returns {{ radios: HTMLInputElement[] }}
   */
  _buildMarketDataTab(panel, marketData) {
    const fields = document.createElement("div");
    fields.className = "config-dialog-fields";
    panel.appendChild(fields);

    const sources = marketData.sources || [];
    const KIND_LABELS = { yfinance: "YFinance", tws: "TWS", ib_gateway: "IB Gateway" };

    const radios = sources.map((s, i) =>
      this._buildRadio(
        fields, KIND_LABELS[s.kind] || s.kind, "config-market-data-active",
        `config-market-data-source-${i}`, s.kind, s.kind === marketData.active
      )
    );

    const note = document.createElement("p");
    note.className = "config-note";
    // Fix G (review-fix-wave, 2026-08-14): a differenza di llms.json/
    // aichat.json (letti una sola volta all'avvio, cache in stato Rust),
    // market_data.json è letto FRESCO a ogni nuovo processo Python
    // (market_data_config.build_data_source, import di server.py -- un
    // nuovo processo per il canale /markets o per ogni click di "Test
    // connessione", mai un riavvio dell'intero orchestrator). Testo
    // precedente ("richiedono il riavvio dell'orchestrator") era sbagliato.
    note.textContent = "Le modifiche alla fonte dati mercato hanno effetto dalla prossima apertura di /markets o dal prossimo Test connessione — non serve riavviare l'orchestrator.";
    panel.appendChild(note);

    // Bottone "Test connessione" — testa SEMPRE la fonte ATTUALMENTE
    // SALVATA su disco (market_data.json), non la radio appena cliccata e
    // non ancora salvata (vedi doc-comment di ClientMsg::TestMarketDataSource
    // in protocol/src/lib.rs e task-12-brief.md).
    const testBtn = document.createElement("button");
    testBtn.type = "button";
    testBtn.className = "config-test-btn";
    testBtn.textContent = "Test connessione (fonte salvata)";
    panel.appendChild(testBtn);

    const testResult = document.createElement("p");
    testResult.className = "config-note";
    panel.appendChild(testResult);

    testBtn.addEventListener("click", () => this._runMarketDataTest(testBtn, testResult));

    return { radios };
  }

  /**
   * Bottone "Test connessione" nel tab Fonte Dati Mercato. Apre una
   * LareWsClient dedicata e usa-e-getta (mai quella del pannello
   * principale — questa finestra /config non ne possiede una), invia
   * ClientMsg::TestMarketDataSource, attende UNA risposta correlata per
   * id, poi si disconnette sempre (successo, errore, o socket caduto).
   * Mirror letterale di runExpand()/library-expand in window.js.
   *
   * @param {HTMLButtonElement}  btn
   * @param {HTMLParagraphElement} resultEl
   */
  async _runMarketDataTest(btn, resultEl) {
    btn.disabled = true;
    resultEl.textContent = "Verifica in corso…";

    // Guardia coerente con tutti gli altri usi di this._invoke in questo
    // file (vedi _loadInitial sopra): senza try/catch un _invoke assente o
    // rifiutato lancerebbe dentro questo handler async, lasciando il
    // bottone bloccato su "Verifica in corso…" per sempre — un fallimento
    // peggiore di quello che la guardia onStatus qui sotto previene già.
    let token = "";
    if (this._invoke) {
      try {
        token = (await this._invoke("get_lare_token")) ?? "";
      } catch (e) {
        console.error("[config-dialog] get_lare_token error:", e);
        resultEl.textContent = "❌ Impossibile ottenere il token.";
        btn.disabled = false;
        return;
      }
    }
    const requestId = crypto.randomUUID();
    let settled = false;

    const client = new LareWsClient({
      token,
      channel: "config-market-data-test",
      // Stessa guardia usata da runExpand()/library-expand: senza questo
      // handler una connessione caduta prima della risposta lascerebbe il
      // bottone bloccato su "Verifica in corso…" per sempre.
      onStatus: (status) => {
        if (!isConnectionFailureStatus(status, settled)) return;
        settled = true;
        resultEl.textContent = "❌ Connessione al servizio persa.";
        btn.disabled = false;
        client.disconnect();
      },
      onMessage: (msg) => {
        if (msg.type === "server_info") {
          client.sendTestMarketDataSource(requestId);
          return;
        }
        if (msg.type !== "market_data_source_test_result" || msg.id !== requestId || settled) return;
        settled = true;
        resultEl.textContent = (msg.ok ? "✅ " : "❌ ") + msg.message;
        btn.disabled = false;
        client.disconnect();
      },
    });
    client.connect();
  }

  /**
   * Build a label + radio-button row. Mirror of `_buildCheckbox`, with a
   * shared `name` so multiple rows form one mutually-exclusive group.
   *
   * @param {HTMLElement} parent
   * @param {string}      labelText
   * @param {string}      name      Shared `name` attribute for the radio group.
   * @param {string}      id        Input element id (also used for label.htmlFor).
   * @param {string}      value
   * @param {boolean}     checked
   * @returns {HTMLInputElement}
   */
  _buildRadio(parent, labelText, name, id, value, checked) {
    const row = document.createElement("div");
    row.className = "config-field-row";
    parent.appendChild(row);

    const label = document.createElement("label");
    label.className = "config-field-label";
    label.htmlFor = id;
    label.textContent = labelText;
    row.appendChild(label);

    const input = document.createElement("input");
    input.type = "radio";
    input.id = id;
    input.name = name;
    input.value = value;
    input.checked = !!checked;
    row.appendChild(input);

    return input;
  }

  /**
   * Build the "Action key" field with a click-to-capture input.
   *
   * Clicking the capture button focuses a hidden input; the next key combo
   * pressed is converted to an accelerator string and shown.  This is safer
   * than a plain text field because it never captures an invalid string.
   *
   * @param {HTMLElement} parent
   * @param {string}      currentKey
   * @returns {HTMLElement}  The display element (carries `dataset.accelerator`).
   */
  _buildActionKeyField(parent, currentKey) {
    const row = document.createElement("div");
    row.className = "config-field-row";
    parent.appendChild(row);

    const label = document.createElement("label");
    label.className = "config-field-label";
    label.textContent = "Tasto azione";
    row.appendChild(label);

    const right = document.createElement("div");
    right.className = "config-field-value config-capture-row";
    row.appendChild(right);

    // Display badge — shows the current/captured accelerator.
    const display = document.createElement("span");
    display.className = "config-key-badge";
    display.textContent = currentKey;
    display.dataset.accelerator = currentKey;
    right.appendChild(display);

    const hint = document.createElement("span");
    hint.className = "config-capture-hint";
    hint.textContent = "Click then press combo";
    right.appendChild(hint);

    // Hidden input used only to capture keydown events.
    const captureInput = document.createElement("input");
    captureInput.type = "text";
    captureInput.readOnly = true;
    captureInput.className = "config-capture-input";
    captureInput.setAttribute("aria-label", "Key capture input — click then press the key combination");
    right.appendChild(captureInput);

    // Activate capture on click.
    display.addEventListener("click", () => captureInput.focus());
    hint.addEventListener("click",    () => captureInput.focus());

    captureInput.addEventListener("keydown", (e) => {
      e.preventDefault();
      e.stopPropagation();

      const accel = keyEventToAccelerator(e);
      if (!accel) return; // modifier-only — wait for real key

      display.textContent = accel;
      display.dataset.accelerator = accel;
      captureInput.blur();
    });

    return display; // returned so saveBtn can read dataset.accelerator
  }

  /**
   * Build a simple label + input row.
   *
   * @param {HTMLElement} parent
   * @param {string}      labelText
   * @param {string}      fieldId     (used for label htmlFor — no sensitive data)
   * @param {string}      inputId
   * @param {string}      type        "text" | "number"
   * @param {string}      value
   * @returns {HTMLInputElement}
   */
  _buildField(parent, labelText, fieldId, inputId, type, value) {
    const row = document.createElement("div");
    row.className = "config-field-row";
    parent.appendChild(row);

    const label = document.createElement("label");
    label.className = "config-field-label";
    label.htmlFor = inputId;
    // labelText is a static string from our code — safe to assign as text.
    label.textContent = labelText;
    row.appendChild(label);

    const input = document.createElement("input");
    input.type = type;
    input.id = inputId;
    input.className = "config-field-input";
    // value comes from the backend — assign as value property (not HTML attribute).
    input.value = value;
    row.appendChild(input);

    return input;
  }

  /**
   * Build a labelled range slider with a live percentage readout.
   *
   * @param {HTMLElement} parent
   * @param {string}      labelText
   * @param {string}      fieldId    (non usato oggi — mantenuto per simmetria con _buildField)
   * @param {string}      inputId
   * @param {number}      min
   * @param {number}      max
   * @param {number}      step
   * @param {number}      value      Valore iniziale, già in unità dello slider (es. 0-100).
   * @returns {{ input: HTMLInputElement, getValue: () => number }}
   */
  _buildSliderField(parent, labelText, fieldId, inputId, min, max, step, value) {
    const row = document.createElement("div");
    row.className = "config-field-row";
    parent.appendChild(row);

    const label = document.createElement("label");
    label.className = "config-field-label";
    label.htmlFor = inputId;
    label.textContent = labelText;
    row.appendChild(label);

    const wrap = document.createElement("div");
    wrap.className = "config-slider-wrap";
    row.appendChild(wrap);

    const input = document.createElement("input");
    input.type = "range";
    input.id = inputId;
    input.className = "config-slider";
    input.min = String(min);
    input.max = String(max);
    input.step = String(step);
    input.value = String(value);
    wrap.appendChild(input);

    const readout = document.createElement("span");
    readout.className = "config-slider-value";
    readout.textContent = `${value}%`;
    wrap.appendChild(readout);

    input.addEventListener("input", () => {
      readout.textContent = `${input.value}%`;
    });

    return { input, getValue: () => Number(input.value) };
  }

  /**
   * Build the colour field: a grid of curated swatches plus the native
   * `<input type="color">` picker for custom colours.
   *
   * Swatches are the robust path (fully in-overlay); the native picker opens
   * the OS colour dialog, which is a bonus but can misbehave over an
   * always-on-top window — if so, the swatches still work.
   *
   * @param {HTMLElement} parent
   * @param {string}      currentColor  hex string, e.g. "#FFFFFF"
   * @returns {{ getValue: () => string }}
   */
  _buildColorField(parent, currentColor) {
    const row = document.createElement("div");
    row.className = "config-field-row";
    parent.appendChild(row);

    const label = document.createElement("label");
    label.className = "config-field-label";
    label.textContent = "Colore";
    row.appendChild(label);

    const wrap = document.createElement("div");
    wrap.className = "config-color-wrap";
    row.appendChild(wrap);

    // Curated terminal-friendly palette.
    const SWATCHES = [
      "#FFFFFF", "#C8C8C8", "#00FF66", "#33FF99", "#00E5FF", "#5AA0FF",
      "#FFB000", "#FF8800", "#FF5555", "#FF55FF", "#FFFF66", "#AAFF33",
    ];

    let selected = (currentColor || "#FFFFFF").toUpperCase();
    const swatchEls = [];

    const swatchRow = document.createElement("div");
    swatchRow.className = "config-swatches";
    wrap.appendChild(swatchRow);

    const native = document.createElement("input");
    native.type = "color";
    native.className = "config-color-native";
    native.title = "Colore personalizzato";

    const isHex6 = (c) => /^#[0-9A-F]{6}$/i.test(c);

    const setSelected = (color) => {
      selected = color.toUpperCase();
      for (const s of swatchEls) {
        s.classList.toggle("selected", s.dataset.color === selected);
      }
      if (isHex6(selected)) native.value = selected.toLowerCase();
    };

    for (const c of SWATCHES) {
      const sw = document.createElement("button");
      sw.type = "button";
      sw.className = "config-swatch";
      sw.dataset.color = c;
      sw.style.background = c;
      sw.title = c;
      sw.addEventListener("click", () => setSelected(c));
      swatchRow.appendChild(sw);
      swatchEls.push(sw);
    }

    native.value = isHex6(selected) ? selected.toLowerCase() : "#ffffff";
    native.addEventListener("input", () => setSelected(native.value));
    wrap.appendChild(native);

    setSelected(selected);

    return { getValue: () => selected };
  }

  /**
   * Build the font field: a dropdown of monospace fonts actually available to
   * the webview, detected via `document.fonts.check()` over a curated list of
   * common monospace families.  The current font is always included.
   *
   * @param {HTMLElement} parent
   * @param {string}      currentFont
   * @returns {HTMLSelectElement}
   */
  _buildFontSelect(parent, currentFont) {
    const row = document.createElement("div");
    row.className = "config-field-row";
    parent.appendChild(row);

    const label = document.createElement("label");
    label.className = "config-field-label";
    label.htmlFor = "config-font";
    label.textContent = "Font";
    row.appendChild(label);

    const select = document.createElement("select");
    select.id = "config-font";
    select.className = "config-field-input";
    row.appendChild(select);

    // Curated monospace candidates across Windows/macOS/Linux.
    const CANDIDATES = [
      "Consolas", "Cascadia Code", "Cascadia Mono", "Courier New",
      "Lucida Console", "JetBrains Mono", "Fira Code", "Fira Mono",
      "Source Code Pro", "Hack", "Inconsolata", "IBM Plex Mono",
      "Roboto Mono", "Ubuntu Mono", "DejaVu Sans Mono", "Liberation Mono",
      "Menlo", "Monaco", "SF Mono", "Noto Sans Mono",
    ];

    const available = new Set();
    for (const f of CANDIDATES) {
      try {
        if (document.fonts && document.fonts.check(`16px "${f}"`)) available.add(f);
      } catch {
        /* document.fonts.check unsupported — skip */
      }
    }
    // Always offer the current font, even if detection missed it.
    if (currentFont) available.add(currentFont);
    if (available.size === 0) available.add("Consolas");

    const sorted = [...available].sort((a, b) => a.localeCompare(b));
    for (const f of sorted) {
      const opt = document.createElement("option");
      opt.value = f;
      opt.textContent = f;
      opt.style.fontFamily = `"${f}", monospace`;
      if (f === currentFont) opt.selected = true;
      select.appendChild(opt);
    }

    return select;
  }

  /**
   * Build the position <select> row.
   *
   * @param {HTMLElement} parent
   * @param {string}      current   snake_case value matching serde enum
   * @returns {HTMLSelectElement}
   */
  _buildPositionSelect(parent, current) {
    const row = document.createElement("div");
    row.className = "config-field-row";
    parent.appendChild(row);

    const label = document.createElement("label");
    label.className = "config-field-label";
    label.htmlFor = "config-position";
    label.textContent = "Posizione";
    row.appendChild(label);

    const select = document.createElement("select");
    select.id = "config-position";
    select.className = "config-field-input";
    row.appendChild(select);

    // Options match the serde snake_case values of the Position enum.
    const OPTIONS = [
      { value: "center",        label: "Centro (default)" },
      { value: "bottom_center", label: "Basso-centro" },
      { value: "near_mouse",    label: "Vicino al mouse" },
    ];

    for (const opt of OPTIONS) {
      const optEl = document.createElement("option");
      optEl.value = opt.value;
      optEl.textContent = opt.label;
      if (opt.value === current) optEl.selected = true;
      select.appendChild(optEl);
    }

    return select;
  }

  /**
   * Build the activity indicator <select> row.
   *
   * Mirror of `_buildPositionSelect`.  Options match the serde snake_case
   * values of the `ActivityIndicator` enum in `config.rs`.
   *
   * @param {HTMLElement} parent
   * @param {string}      current   snake_case value matching serde enum
   * @returns {HTMLSelectElement}
   */
  _buildActivitySelect(parent, current) {
    const row = document.createElement("div");
    row.className = "config-field-row";
    parent.appendChild(row);

    const label = document.createElement("label");
    label.className = "config-field-label";
    label.htmlFor = "config-activity";
    label.textContent = "Indicatore";
    row.appendChild(label);

    const select = document.createElement("select");
    select.id = "config-activity";
    select.className = "config-field-input";
    row.appendChild(select);

    // Options match the serde snake_case values of the ActivityIndicator enum.
    const OPTIONS = [
      { value: "title",  label: "Accanto al titolo (default)" },
      { value: "status", label: "Badge di stato" },
      { value: "prompt", label: "Prompt pulsante" },
    ];

    for (const opt of OPTIONS) {
      const optEl = document.createElement("option");
      optEl.value = opt.value;
      optEl.textContent = opt.label;
      if (opt.value === current) optEl.selected = true;
      select.appendChild(optEl);
    }

    return select;
  }

  /**
   * Build a label + checkbox row.
   *
   * @param {HTMLElement} parent
   * @param {string}      labelText
   * @param {string}      id        Input element id (also used for label.htmlFor).
   * @param {boolean}     checked   Initial checked state.
   * @returns {HTMLInputElement}
   */
  _buildCheckbox(parent, labelText, id, checked) {
    const row = document.createElement("div");
    row.className = "config-field-row";
    parent.appendChild(row);

    const label = document.createElement("label");
    label.className = "config-field-label";
    label.htmlFor = id;
    label.textContent = labelText;
    row.appendChild(label);

    const input = document.createElement("input");
    input.type = "checkbox";
    input.id = id;
    // Use !! to coerce undefined/null to false (e.g. if cfg.web_search_enabled is absent).
    input.checked = !!checked;
    row.appendChild(input);

    return input;
  }
}
