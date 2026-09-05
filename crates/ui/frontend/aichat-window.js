// aichat-window.js — finestra-chat AI Chat (webview Tauri, Slice 1a-ui-B).
// NON apre una WS propria: riceve aggiornamenti via eventi Tauri da app.js
// (connessione WS primaria) e gli rimanda input/consenso/chiusura. app.js parla
// col canale `aichat` dell'orchestrator. Stesso disaccoppiamento delle finestre plugin.
import { messageLine, rosterText, consentPrompt, isSelf, historyEntries, chatWindowTitle, peerLostText } from "./aichat-view.mjs";
import { shareConsentPrompt } from "./share-view.mjs";
// admission.mjs (Task 9): logica PURA del flusso di ammissione (testo del gate 1 +
// stato del pulsante di re-request). Niente DOM lì dentro → la importiamo qui dove
// il DOM invece vive.
import { joinPromptText, rerequestState, removeResolvedCandidate } from "./admission.mjs";

const tauriEvent = window.__TAURI__?.event;
const invoke = window.__TAURI__?.core?.invoke;

const transcript = document.getElementById("transcript");
const rosterEl = document.getElementById("roster");
const titlebarTextEl = document.getElementById("titlebar-text");
const consentEl = document.getElementById("consent");
const consentText = document.getElementById("consent-text");
const shareConsentEl   = document.getElementById("share-consent");
const shareConsentText = document.getElementById("share-consent-text");
const msgInput = document.getElementById("msg");

// ── Elementi DOM del modello di ammissione (Task 10) ───────────────────────
const gate1El = document.getElementById("gate1");
const gate1Text = document.getElementById("gate1-text");
const gate2El = document.getElementById("gate2");
const gate2Text = document.getElementById("gate2-text");
const pendingEl = document.getElementById("pending-banner");
const pendingText = document.getElementById("pending-text");
const rejectedEl = document.getElementById("rejected-banner");
const rerequestBtn = document.getElementById("rerequest-btn");

let pendingPeer = null; // peer_label in attesa di consenso
let pendingShare = null; // { share_id } dell'offerta Share in attesa di consenso
const myBase = "";      // base etichetta locale (non nota lato finestra → no self-highlight)

// Etichetta di QUESTA macchina in questa stanza (arriva via `aichat:self`).  Serve a
// non annunciare "sei entrato" due volte: una come system-line dedicata su
// `aichat:admitted`, l'altra (sbagliata) come diff del roster se il nostro stesso
// nome comparisse nella lista "presenti" appena ammessi.
let selfLabel = null;

// Etichette già viste nel roster: permette di rilevare, per differenza, chi è
// APPENA entrato quando arriva un `aichat:roster` aggiornato. `null` finché non
// abbiamo ricevuto un primo roster: senza un roster precedente non c'è nulla da
// diffare, e trattarlo come "tutti entrati ora" annuncerebbe l'intera stanza.
let lastRosterParticipants = null;

// Coda delle richieste di ammissione (gate 2) in attesa del voto dell'umano di
// QUESTA macchina. Più candidati possono chiedere di entrare mentre l'umano non ha
// ancora risposto al primo: li accodiamo e mostriamo un banner alla volta.
let admissionQueue = [];

// Scadenza (epoch ms) del cooldown di re-request dopo un rifiuto; alimenta
// `rerequestState` insieme a `Date.now()`. `null` = nessun cooldown attivo.
let retryAtMs = null;
// Timer che aggiorna il countdown del pulsante "Chiedi di entrare" ogni secondo.
let rerequestTimer = null;

function addLine({ from_label, text, display_name, is_ai }) {
  const line = messageLine({ from_label, text, display_name, is_ai });
  const div = document.createElement("div");
  // isSelf vuole l'etichetta GREZZA (es. "skimble-human"), non `line.label` — che
  // dopo la risoluzione del nickname può essere un nome proprio ("Maurizio") e non
  // inizia mai con `${myBase}-`, facendo fallire lo stile "miei messaggi" (bug
  // confermato, brief Task 9 Step 5).
  div.className = "line" + (isSelf(from_label, myBase) ? " self" : "") + (line.is_ai ? " ai" : "");
  const lbl = document.createElement("span");
  lbl.className = "lbl";
  lbl.textContent = line.label;             // textContent → niente XSS
  const txt = document.createElement("span");
  txt.textContent = line.text;
  div.appendChild(lbl);
  div.appendChild(txt);
  transcript.appendChild(div);
  transcript.scrollTop = transcript.scrollHeight;
}

function emit(event, payload) {
  return tauriEvent?.emit(event, payload);
}

/**
 * Aggiunge una riga di sistema (corsivo, colore muted — vedi CSS `.line.system`)
 * al trascritto. Usata per avvisi LIVE (peer sparito/entrato, ingresso avvenuto):
 * transitori come `peer_lost`, MAI persistiti — `aichat:history` sostituisce
 * `transcript.innerHTML` per intero, quindi un replay li fa sparire naturalmente.
 */
function addSystemLine(text) {
  const div = document.createElement("div");
  div.className = "line system";
  div.textContent = text;
  transcript.appendChild(div);
  transcript.scrollTop = transcript.scrollHeight;
}

// ── Eventi DA app.js ──────────────────────────────────────────────
tauriEvent?.listen("aichat:msg", (e) => addLine(e.payload || {}));
tauriEvent?.listen("aichat:roster", (e) => {
  const participants = e.payload?.participants || [];
  rosterEl.textContent = rosterText(participants);
  // Notifica "X è entrato in chat.": diff fra il roster precedente e questo. Guardie:
  //  - `lastRosterParticipants === null` → primo roster mai visto, niente da diffare
  //    (altrimenti annunceremmo "entrata" l'intera stanza già presente).
  //  - escludiamo `selfLabel`: la NOSTRA ammissione ha già una riga dedicata su
  //    `aichat:admitted` ("Sei entrato in chat.") — evita il doppio annuncio nel caso
  //    il roster che ci ammette raggiunga questa finestra prima/insieme all'evento.
  if (lastRosterParticipants) {
    const added = participants.filter((p) => !lastRosterParticipants.includes(p));
    for (const label of added) {
      if (label !== selfLabel) addSystemLine(`${label} è entrato in chat.`);
    }
  }
  lastRosterParticipants = participants;
});
tauriEvent?.listen("aichat:history", (e) => {
  // Catch-up/riapertura: svuota il trascritto e ri-renderizza tutto in blocco.
  // Niente merge/dedup — evita duplicati se il dump si sovrappone a righe già viste.
  transcript.innerHTML = "";
  for (const entry of historyEntries(e.payload?.entries)) {
    addLine(entry);
  }
});
tauriEvent?.listen("aichat:self", (e) => {
  // Titolo finestra: "AICHAT - <mia-etichetta>" — così si distingue a colpo d'occhio
  // la propria finestra-chat senza dover leggere il roster "presenti".
  selfLabel = e.payload?.label || null;
  titlebarTextEl.textContent = chatWindowTitle(selfLabel);
});
tauriEvent?.listen("aichat:join-request", (e) => {
  pendingPeer = e.payload?.peer_label || null;
  consentText.textContent = consentPrompt(pendingPeer ?? "?");
  consentEl.classList.add("show");
});
tauriEvent?.listen("aichat:share-request", (e) => {
  const { share_id, from_label, doc_name, size_bytes } = e.payload ?? {};
  pendingShare = { share_id };
  shareConsentText.textContent = shareConsentPrompt(from_label, doc_name, size_bytes);
  shareConsentEl.classList.add("show");
});
tauriEvent?.listen("aichat:peer_lost", (e) => {
  addSystemLine(peerLostText(e.payload?.label));
});

// ── Ammissione alla stanza (Task 10) ───────────────────────────────
// Vedi Docs/superpowers/plans/2026-07-03-aichat-admission.md. Quattro momenti:
//   gate 1 (nuovo arrivato decide se provare)  → banner #gate1
//   pending (voto dei presenti in corso)       → banner #pending-banner + input OFF
//   gate 2 (presente vota un candidato)        → banner #gate2 (in coda se più di uno)
//   ammesso/rifiutato                          → input ON, oppure banner + re-request

/** Ferma il timer del countdown di re-request, se attivo. Idempotente. */
function stopRerequestTimer() {
  if (rerequestTimer !== null) {
    clearInterval(rerequestTimer);
    rerequestTimer = null;
  }
}

tauriEvent?.listen("aichat:join-prompt", (e) => {
  // Prima del gate 1 non siamo (ancora) nella stanza: l'input resta disabilitato
  // finché non arriva `aichat:admitted` (stessa ragione del blocco in "pending" —
  // previene lo scrivere a vuoto prima di sapere se saremo ammessi).
  msgInput.disabled = true;
  gate1Text.textContent = joinPromptText(e.payload?.present);
  gate1El.classList.add("show");
});
document.getElementById("gate1-yes").addEventListener("click", () => {
  gate1El.classList.remove("show");
  emit("aichat:join-decision", { accept: true });
});
document.getElementById("gate1-no").addEventListener("click", () => {
  gate1El.classList.remove("show");
  emit("aichat:join-decision", { accept: false });
  addSystemLine("Hai annullato l'ingresso in chat.");
});

tauriEvent?.listen("aichat:pending", (e) => {
  stopRerequestTimer();
  rejectedEl.classList.remove("show"); // un nuovo tentativo sostituisce il banner di rifiuto
  const present = e.payload?.present || [];
  pendingText.textContent = `In attesa di ammissione da ${present.join(", ")}…`;
  pendingEl.classList.add("show");
  msgInput.disabled = true; // previene il "saluto perso": non si scrive finché non ammessi
});

tauriEvent?.listen("aichat:admitted", () => {
  stopRerequestTimer();
  pendingEl.classList.remove("show");
  rejectedEl.classList.remove("show");
  msgInput.disabled = false;
  addSystemLine("Sei entrato in chat.");
});

// Aggiorna testo/stato del pulsante "Chiedi di entrare" secondo `rerequestState`
// (admission.mjs), e si autodistrugge quando il cooldown scade.
function updateRerequestButton() {
  const { enabled, secondsLeft } = rerequestState(Date.now(), retryAtMs);
  rerequestBtn.disabled = !enabled;
  rerequestBtn.textContent = enabled ? "Chiedi di entrare" : `Chiedi di entrare (${secondsLeft}s)`;
  if (enabled) stopRerequestTimer();
}

tauriEvent?.listen("aichat:rejected", (e) => {
  pendingEl.classList.remove("show");
  msgInput.disabled = true; // il rifiuto non ci fa entrare: restiamo fuori dalla stanza
  const retrySecs = e.payload?.retry_after_secs ?? 0;
  retryAtMs = Date.now() + retrySecs * 1000;
  rejectedEl.classList.add("show");
  updateRerequestButton();
  stopRerequestTimer(); // scaduto un eventuale timer precedente prima di ripartire
  rerequestTimer = setInterval(updateRerequestButton, 1000);
});
rerequestBtn.addEventListener("click", () => {
  if (rerequestBtn.disabled) return;
  emit("aichat:request-admission", {});
  // Non nascondiamo subito il banner "rifiutato": aspettiamo che il server confermi
  // il nuovo tentativo con `aichat:pending` (che lo farà sparire da solo, sopra).
});

// Gate 2: uno o più candidati chiedono di entrare mentre siamo già in stanza.
// Accodati e mostrati UNO alla volta (niente banner sovrapposti).
function showNextAdmissionRequest() {
  if (admissionQueue.length === 0) {
    gate2El.classList.remove("show");
    return;
  }
  gate2Text.textContent = `${admissionQueue[0]} chiede di entrare in chat.`;
  gate2El.classList.add("show");
}
tauriEvent?.listen("aichat:admission-request", (e) => {
  const candidate = e.payload?.candidate;
  if (!candidate) return;
  admissionQueue.push(candidate);
  if (admissionQueue.length === 1) showNextAdmissionRequest(); // altrimenti già in coda
});
// FIX #7 (review 2026-07-03): il server ci dice che un voto si è concluso
// (ammesso/rifiutato altrove, o candidato sparito): togliamo quel candidato dalla
// coda del gate 2 così il banner "ammetti X?" non resta appeso su un voto ormai
// deciso (dove il click sarebbe un no-op, scartato dal server come stale). Se il
// candidato risolto era quello IN TESTA (banner attualmente mostrato), ridisegna
// per mostrare il prossimo o chiudere il banner.
tauriEvent?.listen("aichat:admission-resolved", (e) => {
  const candidate = e.payload?.candidate;
  if (!candidate) return;
  const front = admissionQueue[0];
  admissionQueue = removeResolvedCandidate(admissionQueue, candidate);
  if (admissionQueue[0] !== front) showNextAdmissionRequest();
});
function voteCurrentAdmission(accept) {
  const candidate = admissionQueue.shift();
  if (candidate) emit("aichat:admission-vote", { candidate, accept });
  showNextAdmissionRequest();
}
document.getElementById("gate2-yes").addEventListener("click", () => voteCurrentAdmission(true));
document.getElementById("gate2-no").addEventListener("click", () => voteCurrentAdmission(false));

// ── Input umano → app.js ──────────────────────────────────────────
msgInput.addEventListener("keydown", (ev) => {
  if (ev.key === "Enter" && !ev.shiftKey) {
    ev.preventDefault();
    const text = msgInput.value.trim();
    if (text) { emit("aichat:send", { text }); msgInput.value = ""; }
  }
});

document.getElementById("consent-yes").addEventListener("click", () => {
  if (pendingPeer) emit("aichat:consent", { peer_label: pendingPeer, accept: true });
  consentEl.classList.remove("show"); pendingPeer = null;
});
document.getElementById("consent-no").addEventListener("click", () => {
  if (pendingPeer) emit("aichat:consent", { peer_label: pendingPeer, accept: false });
  consentEl.classList.remove("show"); pendingPeer = null;
});

document.getElementById("share-consent-yes").addEventListener("click", () => {
  if (pendingShare) emit("aichat:share-consent", { share_id: pendingShare.share_id, accept: true });
  shareConsentEl.classList.remove("show"); pendingShare = null;
});
document.getElementById("share-consent-no").addEventListener("click", () => {
  if (pendingShare) emit("aichat:share-consent", { share_id: pendingShare.share_id, accept: false });
  shareConsentEl.classList.remove("show"); pendingShare = null;
});

async function closeWindow() {
  stopRerequestTimer(); // niente interval orfano dopo la chiusura della webview
  // Attendi che "aichat:closed" sia DAVVERO arrivato ad app.js prima di
  // chiedere la distruzione di questa finestra — altrimenti close_self (che
  // tronca il canale IPC di questa webview) può vincere la corsa contro
  // l'emit, prima fire-and-forget: aiChatGate restava bloccato su
  // live=true per sempre dopo la prima chiusura reale (bug osservato dal
  // vivo 2026-07-29 — bustina mai più accesa per i messaggi successivi).
  await emit("aichat:closed", {});
  invoke?.("close_self").catch(() => {});
}
document.getElementById("close-btn").addEventListener("click", closeWindow);
window.addEventListener("beforeunload", () => {
  stopRerequestTimer();
  emit("aichat:closed", {});
});

// Pronta: app.js rigioca i messaggi bufferizzati prima dell'apertura.
emit("aichat:ready", {});
msgInput.focus();

// ── Trasparenza configurabile (--window-alpha) ─────────────────────────────
// Applica all'avvio e ad ogni salvataggio di /config. L'evento "config:saved"
// è un broadcast globale Tauri (emesso da config-window.js) — raggiunge questa
// finestra anche se è già aperta, senza bisogno di riaprirla (confermato in
// Task 4 su Library).
(async () => {
  if (!invoke) return;
  try {
    const cfg = await invoke("get_config");
    if (cfg && typeof cfg.window_alpha === "number") {
      document.documentElement.style.setProperty("--window-alpha", cfg.window_alpha);
    }
  } catch (e) {
    console.warn("[aichat-window] get_config on startup failed:", e);
  }
})();

tauriEvent?.listen("config:saved", (ev) => {
  const alpha = ev.payload?.window_alpha;
  if (typeof alpha === "number") {
    document.documentElement.style.setProperty("--window-alpha", alpha);
  }
});
