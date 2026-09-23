// config-dialog.js — /config in-overlay dialog for Lare Terminal.
//
// Responsibility (SRP): owns the config dialog lifecycle — build DOM,
// populate fields, handle Save/Cancel/Esc, invoke Tauri commands.
// Does NOT own the main overlay, the WebSocket, or the renderer.
//
// The dialog is an in-overlay <div> panel (not a second Tauri window).
// It reads current config via `get_config`, saves via `set_config`, and
// calls back the caller on close via `onClose` (v1: the cursor's overlay
// used this to re-apply CSS vars in place — that usage is gone with the
// overlay; the dedicated /config window below just closes itself instead).
//
// Usage:
//   import { ConfigDialog } from "./config-dialog.js";
//   const dlg = new ConfigDialog(containerEl, tauriInvoke, onConfigSaved, onClose);
//   dlg.open();
// `onClose` (opzionale) viene invocato in close(): la finestra dedicata /config
// lo usa per chiudere la propria webview (Cancel/Esc/dopo-save).

import { LareWsClient } from "./ws-client.js";
import { isConnectionFailureStatus } from "./expand-prompt.mjs";
import { t } from "./i18n.mjs";

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
      web_search_enabled:  true,
      language:            "it",
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
    title.textContent = t("config.window_title");
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
      { id: "ui",          label: t("config.tab_ui"),          panel: uiPanel },
      { id: "search",      label: t("config.tab_search"),      panel: searchPanel },
      { id: "aichat",      label: t("config.tab_aichat"),      panel: aichatPanel },
      { id: "llm",         label: t("config.tab_llm"),         panel: llmPanel },
      { id: "market-data", label: t("config.tab_market_data"), panel: marketDataPanel },
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
    saveBtn.textContent = t("common.save");
    buttons.appendChild(saveBtn);

    const cancelBtn = document.createElement("button");
    cancelBtn.className = "config-btn config-btn-secondary";
    cancelBtn.textContent = t("common.cancel");
    buttons.appendChild(cancelBtn);

    // ── Event wiring ──
    cancelBtn.addEventListener("click", () => this.close());

    saveBtn.addEventListener("click", async () => {
      errorEl.textContent = "";

      const resultCap = parseInt(searchRefs.resultCapInput.value, 10);
      if (!Number.isFinite(resultCap) || resultCap < 1) {
        errorEl.textContent = t("config.error_result_cap");
        return;
      }
      const maxDepth = parseInt(searchRefs.maxDepthInput.value, 10);
      if (!Number.isFinite(maxDepth) || maxDepth < 1) {
        errorEl.textContent = t("config.error_max_depth");
        return;
      }
      const aichatLabel = aichatRefs.labelInput.value.trim();
      if (aichatLabel.length === 0) {
        errorEl.textContent = t("config.error_aichat_label");
        return;
      }
      const aichatPort = parseInt(aichatRefs.portInput.value, 10);
      if (!Number.isFinite(aichatPort) || aichatPort < 1024 || aichatPort > 65535) {
        errorEl.textContent = t("config.error_aichat_port");
        return;
      }

      const newCfg = {
        web_search_enabled:  uiRefs.webSearchInput.checked,
        window_alpha:        uiRefs.alphaField.getValue() / 100,
        language:            uiRefs.languageSelect.value,
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
   * Build the UI tab content.
   *
   * Task 7 (piano 1): l'overlay F2 è sparito, e con lui ogni campo che
   * pilotava SOLO il suo aspetto/comportamento (tasto d'attivazione,
   * colore/font/dimensione del testo, posizione della finestra, stile
   * dell'indicatore di attività, minuti di inattività) — questa tab resta
   * con le due sole impostazioni condivise da tutte le finestre: ricerca web
   * e trasparenza.
   *
   * @param {HTMLElement} panel  The tab panel element to append into.
   * @param {object}      cfg    Current UI config.
   * @returns {{ webSearchInput, alphaField }}
   */
  _buildUiTab(panel, cfg) {
    const fields = document.createElement("div");
    fields.className = "config-dialog-fields";
    panel.appendChild(fields);

    // Ricerca web interna (toggle).
    const webSearchInput = this._buildCheckbox(
      fields, t("config.web_search"), "config-web-search", cfg.web_search_enabled
    );

    // Trasparenza (alpha) — slider 0-100%, salvato come window_alpha (0.0-1.0).
    const alphaField = this._buildSliderField(
      fields, t("config.transparency"), "window-alpha", "config-window-alpha",
      0, 100, 1, Math.round((cfg.window_alpha ?? 0.87) * 100)
    );

    // Lingua (dropdown con opzioni fisse, nel proprio idioma: it/en/es/de/fr/nl/da/ru/pl).
    const languageSelect = this._buildSelect(
      fields, t("config.language"), "config-language",
      [
        { value: "it", label: "Italiano" },
        { value: "en", label: "English" },
        { value: "es", label: "Español" },
        { value: "de", label: "Deutsch" },
        { value: "fr", label: "Français" },
        { value: "nl", label: "Nederlands" },
        { value: "da", label: "Dansk" },
        { value: "ru", label: "Русский" },
        { value: "pl", label: "Polski" },
      ],
      cfg.language || "it"
    );

    return { webSearchInput, alphaField, languageSelect };
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
      fields, t("config.search_result_cap"), "result-cap", "config-result-cap",
      "number", String(search.result_cap ?? 2000)
    );
    resultCapInput.min = "1";
    resultCapInput.max = "100000";

    // Maximum directory depth explored by /find.
    const maxDepthInput = this._buildField(
      fields, t("config.search_max_depth"), "max-depth", "config-max-depth",
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
      fields, t("config.aichat_enabled"), "config-aichat-enabled", aichat.enabled
    );

    // Etichetta base di questa macchina (diventa "<label_base>-human" sul wire).
    const labelInput = this._buildField(
      fields, t("config.aichat_label"), "aichat-label", "config-aichat-label",
      "text", aichat.label_base ?? "lare"
    );

    // Nickname dell'umano — mostrato in AI Chat al posto di "<label_base>-human".
    // Testo libero (spazi/accenti ok), a differenza di "Etichetta" sopra.
    const displayNameInput = this._buildField(
      fields, t("config.aichat_display_name"), "aichat-display-name", "config-aichat-display-name",
      "text", aichat.display_name ?? ""
    );

    // Nickname dell'AI — normalmente scelto dall'AI stessa la prima volta che
    // AI Chat è attivo (chiede all'utente o si sceglie un nome da sola);
    // resta comunque modificabile a mano qui in ogni momento.
    const aiDisplayNameInput = this._buildField(
      fields, t("config.aichat_ai_display_name"), "aichat-ai-display-name", "config-aichat-ai-display-name",
      "text", aichat.ai_display_name ?? ""
    );

    // Porta TCP/UDP della chat.
    const portInput = this._buildField(
      fields, t("config.aichat_port"), "aichat-port", "config-aichat-port",
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
      fields, t("config.aichat_participates"),
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
      fields, t("config.aichat_autoparticipate"),
      "config-aichat-autoparticipate", aichat.ai_autoparticipate ?? false
    );

    // Nota: queste impostazioni non sono live, a differenza di Search.
    const note = document.createElement("p");
    note.className = "config-note";
    note.textContent = t("config.aichat_restart_note");
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
      note.textContent = t("config.llm_empty_note");
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
    note.textContent = t("config.llm_restart_note");
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
    note.textContent = t("config.market_data_note");
    panel.appendChild(note);

    const testBtn = document.createElement("button");
    testBtn.type = "button";
    testBtn.className = "config-test-btn";
    testBtn.textContent = t("config.market_data_test_btn");
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
    resultEl.textContent = t("config.market_data_testing");

    // Guardia coerente con tutti gli altri usi di this._invoke in questo
    // file (vedi _loadInitial sopra): senza try/catch un _invoke assente o
    // rifiutato lancerebbe dentro questo handler async, lasciando il
    // bottone bloccato su "Verifica in corso…" per sempre — un fallimento
    // peggiore di quello che la guardia onStatus qui sotto previene già.
    let token = "";
    let url = "";
    if (this._invoke) {
      try {
        token = (await this._invoke("get_lare_token")) ?? "";
        // Porta WS da startup.json (2.0), stessa guardia del token sopra.
        url = (await this._invoke("get_ws_endpoint")) ?? "";
      } catch (e) {
        console.error("[config-dialog] get_lare_token/get_ws_endpoint error:", e);
        resultEl.textContent = t("config.market_data_token_err");
        btn.disabled = false;
        return;
      }
    }
    const requestId = crypto.randomUUID();
    let settled = false;

    const client = new LareWsClient({
      url,
      token,
      channel: "config-market-data-test",
      // Stessa guardia usata da runExpand()/library-expand: senza questo
      // handler una connessione caduta prima della risposta lascerebbe il
      // bottone bloccato su "Verifica in corso…" per sempre.
      onStatus: (status) => {
        if (!isConnectionFailureStatus(status, settled)) return;
        settled = true;
        resultEl.textContent = t("config.market_data_conn_lost");
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
   * Build a label + select dropdown row.
   *
   * @param {HTMLElement} parent
   * @param {string}      labelText
   * @param {string}      id         Select element id (also used for label.htmlFor).
   * @param {Array<{value: string, label: string}>} options
   * @param {string}      selectedValue
   * @returns {HTMLSelectElement}
   */
  _buildSelect(parent, labelText, id, options, selectedValue) {
    const row = document.createElement("div");
    row.className = "config-field-row";
    parent.appendChild(row);

    const label = document.createElement("label");
    label.className = "config-field-label";
    label.htmlFor = id;
    label.textContent = labelText;
    row.appendChild(label);

    const select = document.createElement("select");
    select.id = id;
    select.className = "config-field-input";
    for (const opt of options) {
      const optEl = document.createElement("option");
      optEl.value = opt.value;
      optEl.textContent = opt.label;
      if (opt.value === selectedValue) {
        optEl.selected = true;
      }
      select.appendChild(optEl);
    }
    row.appendChild(select);

    return select;
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
