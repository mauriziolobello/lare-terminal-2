// library.js — Archive browser window per Lare Terminal (vista Explorer).
//
// Responsabilità (SRP): mostra il contenuto della Library navigabile come
// un file manager (una cartella per volta + breadcrumb). Gestisce apertura,
// creazione, rinomina ed eliminazione di cartelle e documenti .md.
//
// Tab "Markdown" — navigazione Explorer a cartelle:
//   albero:  invoke("archive_list_tree")  → LibraryTree
//   crea:    invoke("archive_create_folder", { parentRel, name })  → rel_path
//   rinomina:invoke("archive_rename_folder", { folderRel, newName })
//   elimina: invoke("archive_delete_folder", { folderRel })
//   apri:    invoke("archive_open", { file })  → ArchiveDoc
//   cancella:invoke("archive_delete", { file })
//
// Tab "Find" — invariato rispetto alle versioni precedenti:
//   lista:   invoke("list_find")
//   apri:    invoke("open_saved_find_window", { file })
//   cancella:invoke("delete_find", { file })
//
// Contratto sicurezza (invariante di tutto il file):
//   - Tutti i testi da dati utente sono impostati via textContent (MAI innerHTML).
//   - I rel-path sono in dataset (attributo DOM sicuro).
//   - CSS.escape è usato nelle query con [data-*] per prevenire injection CSS.

// ---------------------------------------------------------------------------
// Import del modulo puro library-nav.mjs
//
// Le funzioni importate sono pure (niente DOM/Tauri/I/O): testabili in
// isolamento con `node:test` in library-nav.test.mjs.
// ---------------------------------------------------------------------------
import {
  isValidFolderName,
  isFolderEmpty,
  findNode,
  breadcrumbSegments,
  flattenFolders,
  moveTargets,
} from "./library-nav.mjs";
import { shareTargetList } from "./share-view.mjs";
import { visibleNotesSorted, formatNoteMeta } from "./note-view.mjs";

// ---------------------------------------------------------------------------
// Tauri IPC reference
//
// window.__TAURI__?.core?.invoke: il ? evita crash se il file è aperto
// direttamente in un browser senza runtime Tauri.
// ---------------------------------------------------------------------------
const tauriInvoke = window.__TAURI__?.core?.invoke;
const tauriEvent  = window.__TAURI__?.event;

async function invokeCmd(cmd, args) {
  if (!tauriInvoke) return undefined;
  return tauriInvoke(cmd, args);
}

// ---------------------------------------------------------------------------
// DOM references — raccolte in cima per chiarezza (SRP: separazione tra
// wiring DOM e logica di business)
// ---------------------------------------------------------------------------
const closeBtnEl    = document.getElementById("close-btn");
const openDirBtnEl  = document.getElementById("open-dir-btn");
const listAreaEl    = document.getElementById("list-area");
const tabMarkdownEl = document.getElementById("tab-markdown");
const tabFindEl     = document.getElementById("tab-find");
const tabPluginsEl  = document.getElementById("tab-plugins");
const tabNoteEl     = document.getElementById("tab-note");
const toolbarEl     = document.getElementById("lib-toolbar");
const noteToolbarEl = document.getElementById("note-toolbar");
const breadcrumbEl  = document.getElementById("lib-breadcrumb");
const newFolderBtn  = document.getElementById("new-folder-btn");
const newNoteBtn    = document.getElementById("new-note-btn");
const reloadBtn     = document.getElementById("reload-btn");

// Riferimenti alla modale "Sposta" (Slice 4b)
const moveDialog      = document.getElementById("move-dialog");
const moveDialogTitle = document.getElementById("move-dialog-title");
const moveTargetsEl   = document.getElementById("move-targets");
const moveErrorEl     = document.getElementById("move-error");
const moveConfirmBtn  = document.getElementById("move-confirm");
const moveCancelBtn   = document.getElementById("move-cancel");

// Riferimenti alla modale "Condividi" (Slice 1a-ui)
const shareDialog      = document.getElementById("share-dialog");
const shareDialogTitle = document.getElementById("share-dialog-title");
const shareTargetsEl   = document.getElementById("share-targets");
const shareConfirmBtn  = document.getElementById("share-confirm");
const shareCancelBtn   = document.getElementById("share-cancel");

// ---------------------------------------------------------------------------
// Stato della navigazione Explorer
//
// tree:        l'albero LibraryTree caricato dal backend (null = non ancora)
// currentPath: relPath della cartella corrente; "" = root della Library
//
// Invariante: currentPath è sempre una stringa che corrisponde a un nodo
// esistente nell'albero; se la cartella scompare, si fa fallback a "".
// ---------------------------------------------------------------------------
let tree        = null;
let currentPath = "";

// ---------------------------------------------------------------------------
// Tab state
// ---------------------------------------------------------------------------
// activeTab è il tab correntemente mostrato: "markdown" o "find".
let activeTab = "markdown";

// ---------------------------------------------------------------------------
// Stato della modale "Sposta" (Slice 4b)
//
// moveState è l'unica fonte di verità per la modale aperta:
//   itemRel:  relPath dell'elemento da spostare (file o cartella)
//   isFolder: true se l'elemento è una cartella
//   chosen:   relPath della destinazione selezionata (null = nessuna selezione)
//
// Viene resettato a ogni apertura della modale in openMoveDialog().
// I handler #move-confirm e #move-cancel sono registrati UNA sola volta in
// wireMoveDialog() e leggono questo oggetto: evita il rischio di accumulare
// listener duplicati a ogni apertura (Event Listener per-apertura sarebbe un bug).
// ---------------------------------------------------------------------------
const moveState = {
  itemRel:  "",
  isFolder: false,
  chosen:   null,
};

// Stato della modale "Condividi" (Slice 1a-ui) — stesso pattern di moveState:
// itemRel/docName fissati all'apertura, chosen aggiornato dal click su una riga.
const shareState = {
  itemRel:  "",
  docName:  "",
  chosen:   null,
};

// Ultimo roster ricevuto da app.js (via "library:roster") — aggiornato in
// tempo reale se la finestra è aperta quando arriva, e richiesto esplicitamente
// all'apertura (vedi bootstrap) per non perdere un roster arrivato prima.
let currentRoster = [];

// Macchine raggiungibili per "Condividi" (fix 2026-07-28) — self.peers ∩
// self.links lato Rust, indipendente dall'ammissione alla stanza AI Chat.
// Canale separato da "library:roster" sopra: non condivide stato con la chat.
let currentReachablePeers = [];

// Ultimo snapshot di note ricevuto da app.js (via "library:notes-snapshot" /
// "library:note-upserted") — le note NON si caricano via invoke come
// Markdown/Find/Plugins: arrivano spinte via WS/eventi Tauri (vedi loadNoteTab).
let currentNotes = []; // ultimo snapshot ricevuto da app.js

// La rete peer (network.json, campo `enabled`) è accesa? (FIX 6 della review
// finale di branch.)
//
//   null  = non ancora saputo (o lettura fallita) → non affermiamo nulla
//   true  = abilitata → una lista vuota significa davvero "nessuna nota"
//   false = disabilitata → il Blocco note NON PUÒ funzionare, e una lista vuota
//           non è una lista vuota: è una funzione spenta
//
// Perché serve: con `enabled: false` (che è il DEFAULT) l'orchestrator non
// avvia affatto il servizio, quindi nessun `NotesSnapshot` arriva mai e il tab
// mostrava "Nessuna nota." — indistinguibile da "hai zero note", un vicolo
// cieco silenzioso. Il segnale non esiste sul filo WS; lo leggiamo invece dal
// comando Tauri `get_aichat_settings`, che è già la sorgente di verità usata
// da /config → AI Chat e che (dal FIX 4) legge lo stesso `network.json`
// dell'orchestrator. Tenere `null` distinto da `false` evita di accusare
// "disabilitata" quando è solo la lettura ad essere fallita.
let peerNetworkEnabled = null;

// ---------------------------------------------------------------------------
// Close helper
// ---------------------------------------------------------------------------
async function closeWindow() {
  await invokeCmd("close_self").catch(() => {});
}

closeBtnEl.addEventListener("click", closeWindow);

// ---------------------------------------------------------------------------
// Open-folder button (#open-dir-btn)
//
// Emette un evento Tauri globale `library:open-folder { path }` che app.js
// gestisce inviando `/open <path>` all'orchestrator.
// Sicurezza: il path viene dal backend Rust (trusted), non dal DOM.
// ---------------------------------------------------------------------------
if (openDirBtnEl) {
  openDirBtnEl.addEventListener("click", async () => {
    try {
      // v0.26.0: apre library/documents/ (coerente con la vista Explorer).
      // `library_dir` è mantenuto per altri usi (root del repository).
      const path = await invokeCmd("documents_dir");
      if (path && tauriEvent?.emit) {
        await tauriEvent.emit("library:open-folder", { path });
      }
    } catch (e) {
      console.error("[library] open-dir-btn error:", e);
    }
  });
}

// ---------------------------------------------------------------------------
// Tasto Esc: chiude la finestra.
//
// ATTENZIONE: quando è attiva una rinomina inline, il keydown dell'input
// chiama e.stopPropagation() prima che questo handler venga eseguito, quindi
// Esc durante rinomina annulla la rinomina ma NON chiude la finestra.
// ---------------------------------------------------------------------------
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    e.preventDefault();
    closeWindow();
  }
});

// ---------------------------------------------------------------------------
// Navigazione da tastiera tra le righe (ArrowUp/ArrowDown)
//
// Usa il selettore ".archive-item" che include sia le righe-cartella
// (.archive-item.lib-folder-row) sia le righe-documento (.archive-item).
// Guard: se il focus è su un <input> (es. rinomina inline), non interferisce
// con il caret dell'input.
// ---------------------------------------------------------------------------
document.addEventListener("keydown", (e) => {
  if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;

  // Non rubare frecce all'input di rinomina: il caret si muoverebbe male
  if (document.activeElement?.tagName === "INPUT") return;

  const items = Array.from(listAreaEl.querySelectorAll(".archive-item"));
  if (items.length === 0) return;

  const focused = document.activeElement;
  const idx     = items.indexOf(focused);

  if (e.key === "ArrowDown") {
    e.preventDefault();
    const next = idx < items.length - 1 ? items[idx + 1] : items[0];
    next.focus();
  } else {
    e.preventDefault();
    const prev = idx > 0 ? items[idx - 1] : items[items.length - 1];
    prev.focus();
  }
});

// ---------------------------------------------------------------------------
// Date formatter: converte un timestamp Unix (ms) in stringa leggibile.
// isSeconds=true: il timestamp è in secondi (formato FindEntry.modified).
// ---------------------------------------------------------------------------
function formatDate(ts, isSeconds = false) {
  const ms = isSeconds ? ts * 1000 : ts;
  try {
    return new Date(ms).toLocaleString("it-IT", {
      day:    "2-digit",
      month:  "2-digit",
      year:   "numeric",
      hour:   "2-digit",
      minute: "2-digit",
    });
  } catch {
    return new Date(ms).toLocaleString();
  }
}

// ---------------------------------------------------------------------------
// flashRowError: flash rosso temporaneo su una riga in caso di errore.
// Dura 1.5 s; la riga ritorna normale automaticamente.
// ---------------------------------------------------------------------------
function flashRowError(row) {
  row.classList.add("archive-item--error");
  setTimeout(() => row.classList.remove("archive-item--error"), 1500);
}

// ---------------------------------------------------------------------------
// showError: sostituisce il list-area con un messaggio di errore.
// Usato SOLO per errori di bootstrap (es. archive_list_tree che fallisce):
// quando non c'è una lista da preservare.
// ---------------------------------------------------------------------------
function showError(message) {
  listAreaEl.textContent = "";
  listAreaEl.className = "error";
  listAreaEl.textContent = message;
}

// ---------------------------------------------------------------------------
// wireDeleteButton — due passi di conferma per l'eliminazione di documenti.
//
// Primo click: arma lo stato "Conferma" (3 s); secondo click: esegue.
// Timeout: ripristina lo stato normale.
//
// onSuccess (opzionale): callback asincrona chiamata dopo il delete
//   - Se fornita: chiama onSuccess() invece di rimuovere la riga manualmente.
//     Usata nel contesto Explorer per chiamare reloadCurrentView() e ricevere
//     lo stato aggiornato dal backend (unica fonte di verità).
//   - Se null: comportamento originale (remove() + check empty list).
//     Usato dal tab Find e per retrocompatibilità.
// ---------------------------------------------------------------------------
function wireDeleteButton(deleteBtn, row, file, deleteCmd, onSuccess = null) {
  let confirmTimeout = null;

  function resetConfirm() {
    clearTimeout(confirmTimeout);
    confirmTimeout = null;
    deleteBtn.classList.remove("archive-item-delete--confirm");
    deleteBtn.textContent = "\u{1F5D1}"; // 🗑
    deleteBtn.title = "Elimina";
  }

  function armConfirm() {
    deleteBtn.classList.add("archive-item-delete--confirm");
    deleteBtn.textContent = "Conferma";
    deleteBtn.title = "Clicca ancora per eliminare";
    confirmTimeout = setTimeout(resetConfirm, 3000);
  }

  async function executeDelete() {
    clearTimeout(confirmTimeout);
    confirmTimeout = null;

    try {
      await invokeCmd(deleteCmd, { file });

      if (onSuccess) {
        // Contesto Explorer: ricarica la vista dal backend (verità assoluta)
        await onSuccess();
      } else {
        // Contesto flat list (tab Find): rimuovi la riga manualmente
        row.remove();
        if (!listAreaEl.querySelector(".archive-item")) {
          listAreaEl.className = "empty";
          listAreaEl.textContent =
            activeTab === "find"
              ? "Nessuna ricerca salvata."
              : "Nessun documento archiviato.";
        }
      }
    } catch (e) {
      console.error(`[library] ${deleteCmd} failed:`, e);
      resetConfirm();
      flashRowError(row);
    }
  }

  deleteBtn.addEventListener("click", (e) => {
    e.stopPropagation();
    if (deleteBtn.classList.contains("archive-item-delete--confirm")) {
      executeDelete();
    } else {
      armConfirm();
    }
  });

  // Stoppa la propagazione del dblclick: un doppio-click rapido su 🗑
  // non deve bubblarsi fino al handler dblclick→open della riga.
  deleteBtn.addEventListener("dblclick", (e) => {
    e.stopPropagation();
  });
}

// ---------------------------------------------------------------------------
// wireFolderDeleteButton — due passi di conferma per l'eliminazione cartella.
//
// Logica analoga a wireDeleteButton ma:
//   - chiama archive_delete_folder con { folderRel } (non { file })
//   - dopo il delete (successo o errore): chiama sempre reloadCurrentView()
//     perché il backend è la fonte di verità sul contenuto della cartella.
//
// Il 🗑 è già visibile solo se isFolderEmpty(folder) === true (stima
// ottimistica); se il backend rifiuta (cartella non vuota), flashRowError
// mostra il rosso e la vista viene ricaricata per mostrare il contenuto reale.
// ---------------------------------------------------------------------------
function wireFolderDeleteButton(deleteBtn, row, folderRel) {
  let confirmTimeout = null;

  function resetConfirm() {
    clearTimeout(confirmTimeout);
    confirmTimeout = null;
    deleteBtn.classList.remove("lib-folder-btn--confirm");
    deleteBtn.textContent = "\u{1F5D1}"; // 🗑
    deleteBtn.title = "Elimina cartella";
  }

  function armConfirm() {
    deleteBtn.classList.add("lib-folder-btn--confirm");
    deleteBtn.textContent = "Conferma";
    deleteBtn.title = "Clicca ancora per eliminare";
    confirmTimeout = setTimeout(resetConfirm, 3000);
  }

  async function executeDelete() {
    clearTimeout(confirmTimeout);
    confirmTimeout = null;

    try {
      await invokeCmd("archive_delete_folder", { folderRel });
      // Successo: ricarica la vista (la cartella è scomparsa)
      await reloadCurrentView();
    } catch (e) {
      // Errore tipico: la cartella non era fisicamente vuota (es. contiene
      // file non-.md non visibili nella Library). Flash rosso, poi ricarica
      // per mostrare il contenuto reale.
      console.error("[library] archive_delete_folder failed:", e);
      resetConfirm();
      flashRowError(row);
      await reloadCurrentView();
    }
  }

  deleteBtn.addEventListener("click", (e) => {
    e.stopPropagation();
    if (deleteBtn.classList.contains("lib-folder-btn--confirm")) {
      executeDelete();
    } else {
      armConfirm();
    }
  });

  deleteBtn.addEventListener("dblclick", (e) => {
    e.stopPropagation();
  });
}

// ---------------------------------------------------------------------------
// wireNoteDeleteButton — due passi di conferma per l'eliminazione di una nota
// (Blocco note). Aggiunta post-review: il codice letterale del Task 19 brief
// eseguiva al PRIMO click, inconsistente con wireDeleteButton/
// wireFolderDeleteButton sopra, che armano un stato "Conferma" (3 s) e
// eseguono solo al secondo click entro la finestra.
//
// STESSO MECCANISMO di wireDeleteButton (classe "archive-item-delete--confirm",
// testo "Conferma", timeout 3 s, secondo click esegue) ma NON è un riuso
// diretto: wireDeleteButton è accoppiata a invokeCmd(deleteCmd, { file }) — un
// comando Tauri `invoke` awaitabile con parametro `file`, più un `onSuccess`/
// row.remove() per aggiornare la lista — mentre l'eliminazione di una nota è
// un fire-and-forget tauriEvent.emit("library:note-delete", { id }) (Task 16):
// nessuna eccezione da catturare, nessun reload esplicito (la riga sparisce da
// sola al prossimo library:notes-snapshot/note-upserted con la nota
// tombstoned, filtrata da visibleNotesSorted in renderNoteList). Duplicare
// localmente lo stesso meccanismo è quindi la modifica più fedele e più
// piccola, invece di forzare un riuso che non calza sulla forma dei parametri.
// ---------------------------------------------------------------------------
function wireNoteDeleteButton(deleteBtn, noteId) {
  let confirmTimeout = null;

  function resetConfirm() {
    clearTimeout(confirmTimeout);
    confirmTimeout = null;
    deleteBtn.classList.remove("archive-item-delete--confirm");
    deleteBtn.textContent = "\u{1F5D1}"; // 🗑
    deleteBtn.title = "Elimina";
  }

  function armConfirm() {
    deleteBtn.classList.add("archive-item-delete--confirm");
    deleteBtn.textContent = "Conferma";
    deleteBtn.title = "Clicca ancora per eliminare";
    confirmTimeout = setTimeout(resetConfirm, 3000);
  }

  function executeDelete() {
    clearTimeout(confirmTimeout);
    confirmTimeout = null;
    // Fire-and-forget (v. commento sopra): nessun try/catch da fare qui, a
    // differenza di wireDeleteButton — non c'è un invoke che possa rigettare.
    tauriEvent.emit("library:note-delete", { id: noteId });
  }

  deleteBtn.addEventListener("click", (e) => {
    e.stopPropagation();
    if (deleteBtn.classList.contains("archive-item-delete--confirm")) {
      executeDelete();
    } else {
      armConfirm();
    }
  });

  // Stoppa la propagazione del dblclick, stessa ragione di wireDeleteButton:
  // un doppio-click rapido su 🗑 non deve bubblarsi a un eventuale handler
  // dblclick della riga.
  deleteBtn.addEventListener("dblclick", (e) => {
    e.stopPropagation();
  });
}

// ---------------------------------------------------------------------------
// startRename — rinomina inline di una cartella.
//
// Sostituisce lo span del nome con un <input> precompilato e selezionato.
// Commit: Invio o blur (se il nome è valido tramite isValidFolderName).
// Annulla: Esc (stopPropagation per non chiudere la finestra).
//
// Il nome non valido → flash rosso sull'input, focus ritorna all'input.
// Dopo commit: reloadCurrentView() (resta in currentPath).
// ---------------------------------------------------------------------------
function startRename(row, folder) {
  const nameSpan = row.querySelector(".lib-folder-name");
  if (!nameSpan) return;

  // Crea l'input inline con il nome corrente già selezionato
  const input = document.createElement("input");
  input.type      = "text";
  input.className = "lib-rename-input";
  input.value     = folder.name;

  // Nasconde lo span del nome e inserisce l'input al suo posto
  nameSpan.style.display = "none";
  nameSpan.parentNode.insertBefore(input, nameSpan.nextSibling);

  // Flag per evitare doppio-commit (Enter + blur entrambi potrebbero farlo)
  let done = false;

  input.focus();
  input.select(); // Seleziona tutto: facilita la sovrascrittura rapida

  async function commitRename() {
    if (done) return;
    done = true;

    const newName = input.value.trim();

    if (!isValidFolderName(newName)) {
      // Nome non valido: flash rosso + ripristina focus
      done = false; // Permettiamo un altro tentativo
      input.classList.add("lib-rename-input--error");
      setTimeout(() => {
        input.classList.remove("lib-rename-input--error");
        // input potrebbe essere già rimosso da un blur+commit concorrente;
        // verifichiamo prima di rimettere il focus.
        if (input.isConnected) {
          input.focus();
          input.select();
        }
      }, 600);
      return;
    }

    // Nome valido: rimuovi l'input, mostra il nome originale temporaneamente
    // (sarà rimpiazzato da reloadCurrentView che ricostruisce tutto il DOM)
    input.remove();
    nameSpan.style.display = "";

    try {
      await invokeCmd("archive_rename_folder", {
        folderRel: folder.rel_path,
        newName,
      });
      // La rinomina cambia il rel_path; reloadCurrentView mantiene currentPath
      // (che ora potrebbe non includere il nome vecchio).
      await reloadCurrentView();
    } catch (e) {
      console.error("[library] archive_rename_folder failed:", e);
      flashRowError(row);
      await reloadCurrentView();
    }
  }

  function cancelRename() {
    if (done) return;
    done = true; // Blocca il blur che verrebbe emesso dalla rimozione
    input.remove();
    nameSpan.style.display = "";
    // NON chiamare reloadCurrentView: l'utente ha annullato, la vista è OK
  }

  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      commitRename();
    } else if (e.key === "Escape") {
      // CRUCIALE: stopPropagation evita che l'handler globale Esc chiuda la
      // finestra mentre l'utente sta solo annullando la rinomina.
      e.stopPropagation();
      e.preventDefault();
      cancelRename();
    }
  });

  // Commit su blur: l'utente ha cliccato fuori dall'input
  input.addEventListener("blur", () => {
    if (!done && input.isConnected) {
      commitRename();
    }
  });
}

// ---------------------------------------------------------------------------
// openMoveDialog — apre la modale "Sposta" per un elemento.
//
// Parametri:
//   itemRel:     relPath dell'elemento da spostare (es. "Proj/doc.md")
//   isFolder:    true se l'elemento è una cartella
//   displayName: nome da mostrare nel titolo del dialog (non usato come path)
//
// Flusso:
//   1. Calcola le destinazioni valide con moveTargets() (funzione pura).
//   2. Salva lo stato in moveState (letto dai handler del dialog).
//   3. Popola il titolo e la lista delle destinazioni.
//   4. Apre il dialog con showModal().
//
// Sicurezza: tutti i testi utente sono impostati via textContent (MAI innerHTML).
// ---------------------------------------------------------------------------
function openMoveDialog(itemRel, isFolder, displayName) {
  // Calcola le destinazioni valide tramite la funzione pura (usa l'albero globale)
  const targets = moveTargets(tree, itemRel, isFolder);

  // Aggiorna lo stato condiviso con i handler del dialog
  moveState.itemRel  = itemRel;
  moveState.isFolder = isFolder;
  moveState.chosen   = null; // Nessuna selezione iniziale

  // Imposta il titolo del dialog — textContent per sicurezza (displayName è dati utente)
  moveDialogTitle.textContent = `Sposta “${displayName}” in…`; // "Sposta «nome» in…"

  // Nasconde la riga di errore (potrebbe essere visibile da un'apertura precedente)
  moveErrorEl.style.display = "none";
  moveErrorEl.textContent   = "";

  // Disabilita il pulsante conferma finché l'utente non sceglie una destinazione
  moveConfirmBtn.disabled = true;

  // Pulisce la lista delle destinazioni precedenti
  moveTargetsEl.textContent = "";

  if (targets.length === 0) {
    // Caso raro: nessuna destinazione disponibile (es. unica cartella al livello root)
    const msg = document.createElement("div");
    msg.className   = "move-target";
    msg.textContent = "Nessuna destinazione disponibile.";
    msg.style.color = "var(--text-muted)";
    msg.style.cursor = "default";
    moveTargetsEl.appendChild(msg);
    // Il pulsante conferma resta disabilitato (nessuna scelta possibile)
  } else {
    // Renderizza ogni destinazione come riga selezionabile
    for (const target of targets) {
      const row = document.createElement("div");
      row.className = "move-target" + (target.disabled ? " move-target--disabled" : "");
      row.setAttribute("role", "option");
      row.setAttribute("aria-selected", "false");

      // Indentazione proporzionale alla profondità (14px base + 12px per livello)
      // La root (depth 0) e le cartelle di primo livello (depth 0) hanno stesso indent.
      // Le cartelle annidate ricevono padding crescente per il tree-look.
      const indent = 14 + target.depth * 12;
      row.style.paddingLeft = indent + "px";

      // Etichetta: la root ottiene un'icona speciale per distinguerla visivamente
      if (target.relPath === "") {
        row.textContent = "\u{1F3E0} " + target.name; // 🏠 Documenti (radice)
      } else {
        row.textContent = "\u{1F4C1} " + target.name; // 📁 nome-cartella
      }

      // Riga disabled = il parent corrente dell'elemento da spostare: non è
      // una destinazione valida (spostarcelo sarebbe un no-op), resta solo
      // come ancora visiva per i fratelli rimasti in lista alla loro depth
      // originale (v. commento su moveTargets in library-nav.mjs). Niente
      // aria-selected/click: la riga non è selezionabile.
      if (target.disabled) {
        row.setAttribute("aria-disabled", "true");
        row.textContent += "  (posizione attuale)";
        moveTargetsEl.appendChild(row);
        continue;
      }

      // Memorizza il relPath in dataset per recuperarlo al click di conferma
      row.dataset.targetRel = target.relPath;

      // Click su una riga: seleziona questa destinazione (deseleziona le altre)
      row.addEventListener("click", () => {
        // Rimuovi la selezione da tutte le righe
        moveTargetsEl.querySelectorAll(".move-target--selected").forEach((r) => {
          r.classList.remove("move-target--selected");
          r.setAttribute("aria-selected", "false");
        });

        // Segna questa riga come selezionata
        row.classList.add("move-target--selected");
        row.setAttribute("aria-selected", "true");

        // Salva la scelta nello stato condiviso e abilita il pulsante conferma
        moveState.chosen = target.relPath;
        moveConfirmBtn.disabled = false;
      });

      moveTargetsEl.appendChild(row);
    }
  }

  // Apre il dialog come modale bloccante (blocca il resto della UI con ::backdrop)
  moveDialog.showModal();
}

// ---------------------------------------------------------------------------
// openShareDialog(itemRel, displayName) — apre la modale "Condividi".
//
// A differenza di openMoveDialog (che legge l'albero già caricato in memoria),
// l'elenco macchine viene da currentRoster (aggiornato via evento Tauri da
// app.js) — non serve un invoke qui, solo lo stato già cablato dal bootstrap.
// ---------------------------------------------------------------------------
function openShareDialog(itemRel, displayName) {
  shareState.itemRel = itemRel;
  shareState.docName = displayName;
  shareState.chosen  = null;

  shareDialogTitle.textContent = `Condividi “${displayName}” con…`;
  shareConfirmBtn.disabled = true;
  shareTargetsEl.textContent = "";

  const targets = shareTargetList(currentReachablePeers);

  if (targets.length === 0) {
    const msg = document.createElement("div");
    msg.className    = "move-target";
    msg.textContent  = "Nessuna macchina connessa.";
    msg.style.color  = "var(--text-muted)";
    msg.style.cursor = "default";
    shareTargetsEl.appendChild(msg);
  } else {
    for (const label of targets) {
      const row = document.createElement("div");
      row.className = "move-target";
      row.setAttribute("role", "option");
      row.setAttribute("aria-selected", "false");
      row.style.paddingLeft = "14px";
      row.textContent = "\u{1F5A5} " + label; // 🖥 nome-macchina
      row.dataset.targetLabel = label;

      row.addEventListener("click", () => {
        shareTargetsEl.querySelectorAll(".move-target--selected").forEach((r) => {
          r.classList.remove("move-target--selected");
          r.setAttribute("aria-selected", "false");
        });
        row.classList.add("move-target--selected");
        row.setAttribute("aria-selected", "true");
        shareState.chosen = label;
        shareConfirmBtn.disabled = false;
      });

      shareTargetsEl.appendChild(row);
    }
  }

  shareDialog.showModal();
}

// ---------------------------------------------------------------------------
// wireMoveDialog — collega i handler del dialog (eseguito una sola volta).
//
// I listener sono registrati a bootstrap e leggono moveState al momento del click.
// Questo pattern "singleton listener + stato esterno" evita di accumulare listener
// duplicati ogni volta che il dialog viene aperto (anti-pattern comune).
//
// #move-confirm → invoca archive_move_file o archive_move_folder in base a isFolder.
//   - Successo: chiude il dialog, ricarica la vista.
//   - Errore:   mostra il messaggio in #move-error (NON chiude: l'utente può
//               scegliere un'altra destinazione).
//
// #move-cancel → chiude il dialog (moveDialog.close()).
//
// GOTCHA Esc (advisor): l'Esc nativo del <dialog> emette keydown+cancel.
// L'handler globale a livello document (riga ~122 in library.js) chiude la
// FINESTRA su Escape. Il dialog è nel DOM → il keydown bubbla → la finestra
// verrebbe chiusa insieme al dialog.
// Soluzione: listener keydown sul dialog, bubble phase, stopPropagation su Escape
// SENZA preventDefault → il dialog si chiude normalmente, ma l'evento non raggiunge
// il document handler.
// Il listener cancel è aggiunto per buona misura (harmless), ma NON è il fix carico:
// "cancel" e "keydown" sono eventi diversi e non si intersecano nel flusso.
// ---------------------------------------------------------------------------
function wireMoveDialog() {
  if (!moveDialog) return; // Protezione se l'HTML non include il dialog

  // --- GOTCHA Esc: intercetta il keydown nel dialog prima che bubbi al document ---
  // La cattura "bubble phase" (default) è sufficiente perché il dialog è nel DOM
  // e riceve l'evento prima dell'handler document-level (che è anch'esso bubble).
  // stopPropagation ferma il bubble → document non lo riceve → finestra non si chiude.
  // NON usiamo preventDefault: vogliamo che il browser chiuda il dialog nativamente.
  moveDialog.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      e.stopPropagation(); // Blocca la chiusura della finestra Library
      // NON e.preventDefault(): il dialog si chiude normalmente con Esc
    }
  });

  // Listener aggiuntivo su "cancel" (evento emesso dal browser quando Esc chiude il dialog).
  // Harmless ma esplicito per documentare l'intento.
  moveDialog.addEventListener("cancel", (e) => {
    e.stopPropagation(); // Prevent bubbling (even if cancel doesn't reach document handlers)
  });

  // --- Pulsante "Annulla" ---
  moveCancelBtn.addEventListener("click", () => {
    moveDialog.close();
  });

  // --- Pulsante "Sposta qui" ---
  moveConfirmBtn.addEventListener("click", async () => {
    const { itemRel, isFolder, chosen } = moveState;

    // Sicurezza: non fare nulla se per qualche motivo chosen è null
    if (chosen === null) return;

    try {
      if (isFolder) {
        // Spostamento cartella: archive_move_folder { folderRel, targetParentRel }
        await invokeCmd("archive_move_folder", {
          folderRel:       itemRel,
          targetParentRel: chosen,
        });
      } else {
        // Spostamento documento: archive_move_file { fileRel, targetFolderRel }
        await invokeCmd("archive_move_file", {
          fileRel:         itemRel,
          targetFolderRel: chosen,
        });
      }

      // Successo: chiudi il dialog e ricarica la vista con l'albero aggiornato
      moveDialog.close();
      await reloadCurrentView();

    } catch (e) {
      // Errore dal backend (es. collisione, traversal, cartella inesistente):
      // mostra il messaggio DENTRO il dialog senza chiuderlo.
      // L'utente può scegliere un'altra destinazione o premere Annulla.
      console.error("[library] move failed:", e);
      const msg = typeof e === "string" ? e : (e?.message ?? "Errore sconosciuto.");
      moveErrorEl.textContent   = "⚠ " + msg; // ⚠ messaggio
      moveErrorEl.style.display = "block";
    }
  });
}

// ---------------------------------------------------------------------------
// wireShareDialog — collega i handler del dialog "Condividi" (una sola volta).
//
// Stesso pattern anti-duplicazione-listener di wireMoveDialog. Il gotcha Esc
// (vedi commento di wireMoveDialog) si applica identico: intercetta il keydown
// PRIMA che bubbli all'handler globale che chiuderebbe l'intera finestra.
//
// #share-confirm: calcola size_bytes leggendo il documento via archive_open
// (la Library non tiene la dimensione in memoria — solo titolo/data/rel-path),
// poi emette "library:share-document" verso app.js e chiude SUBITO il dialog
// (fire-and-forget: l'esito arriva più tardi nel pannello del cursore, non qui).
// ---------------------------------------------------------------------------
function wireShareDialog() {
  if (!shareDialog) return;

  shareDialog.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      e.stopPropagation();
    }
  });
  shareDialog.addEventListener("cancel", (e) => {
    e.stopPropagation();
  });

  shareCancelBtn.addEventListener("click", () => {
    shareDialog.close();
  });

  shareConfirmBtn.addEventListener("click", async () => {
    const { itemRel, docName, chosen } = shareState;
    if (chosen === null) return;

    try {
      const doc = await invokeCmd("archive_open", { file: itemRel });
      const sizeBytes = new TextEncoder().encode(doc.content).length;

      if (tauriEvent?.emit) {
        await tauriEvent.emit("library:share-document", {
          rel_path: itemRel,
          doc_name: docName,
          size_bytes: sizeBytes,
          target_label: chosen,
        });
      }
    } catch (e) {
      console.error("[library] share failed:", e);
    }

    shareDialog.close();
  });
}

// ---------------------------------------------------------------------------
// buildFolderRow — costruisce una riga per una sottocartella.
//
// La riga include:
//   📁 [nome]  [✏️]  [🗑]   (🗑 visibile solo se la cartella sembra vuota)
//
// Doppio-click / Invio sulla riga (non sui pulsanti): entra nella cartella.
// ✏️: avvia la rinomina inline via startRename().
// 🗑: due passi di conferma via wireFolderDeleteButton().
//
// La riga usa .archive-item per unificarsi alla navigazione ArrowUp/Down.
// _folderData è usato per startRename dopo reloadCurrentView (vedi newFolderBtn).
// ---------------------------------------------------------------------------
function buildFolderRow(folder) {
  const row = document.createElement("div");
  row.className = "archive-item lib-folder-row";
  row.tabIndex  = 0;
  row.setAttribute("role", "option");
  row.dataset.folderRel = folder.rel_path;
  // Metadati del folder accessibili dopo reloadCurrentView (per la rinomina
  // della cartella appena creata senza un secondo round-trip al backend).
  row._folderData = folder;

  // Icona cartella (non interattiva, decorativa)
  const iconSpan = document.createElement("span");
  iconSpan.className   = "lib-folder-icon";
  iconSpan.textContent = "\u{1F4C1}"; // 📁
  iconSpan.setAttribute("aria-hidden", "true");

  // Nome della cartella (testContent — MAI innerHTML)
  const nameSpan = document.createElement("span");
  nameSpan.className   = "archive-item-title lib-folder-name";
  nameSpan.textContent = folder.name; // textContent — sicuro anche se nome contiene <>

  // Pulsante rinomina ✏️
  const renameBtn = document.createElement("button");
  renameBtn.className   = "lib-folder-btn lib-folder-btn--rename";
  renameBtn.textContent = "✏️"; // ✏️
  renameBtn.title       = "Rinomina cartella";
  renameBtn.setAttribute("aria-label", "Rinomina " + folder.name);

  // Pulsante sposta ↗ (visibile sempre — l'utente sceglie la destinazione nel dialog)
  const moveBtn = document.createElement("button");
  moveBtn.className   = "lib-folder-btn lib-folder-btn--move";
  moveBtn.textContent = "\u{2197}"; // ↗ U+2197 NORTH EAST ARROW
  moveBtn.title       = "Sposta";
  moveBtn.setAttribute("aria-label", "Sposta cartella " + folder.name);

  // Pulsante elimina 🗑 (visibile SOLO se la cartella sembra vuota)
  const deleteBtn = document.createElement("button");
  deleteBtn.className   = "lib-folder-btn lib-folder-btn--delete";
  deleteBtn.textContent = "\u{1F5D1}"; // 🗑
  deleteBtn.title       = "Elimina cartella";
  deleteBtn.setAttribute("aria-label", "Elimina " + folder.name);

  // Stima ottimistica: nasconde il pulsante se la cartella ha contenuto
  // (il backend resta la fonte di verità — se il delete fallisce, flashRowError)
  if (!isFolderEmpty(folder)) {
    deleteBtn.style.visibility = "hidden";
  }

  row.appendChild(iconSpan);
  row.appendChild(nameSpan);
  row.appendChild(renameBtn);
  row.appendChild(moveBtn);
  row.appendChild(deleteBtn);

  // Doppio-click sulla riga (non sui pulsanti figli): entra nella cartella
  row.addEventListener("dblclick", (e) => {
    if (e.target.closest("button")) return; // Il click era su ✏️ o 🗑, ignora
    currentPath = folder.rel_path;
    renderCurrent();
  });

  // Enter sulla riga (non sui pulsanti): entra nella cartella
  row.addEventListener("keydown", (e) => {
    if (e.target !== row) return; // Solo quando la riga stessa ha il focus
    if (e.key === "Enter") {
      e.preventDefault();
      currentPath = folder.rel_path;
      renderCurrent();
    }
  });

  // Wire del pulsante rinomina
  renameBtn.addEventListener("click", (e) => {
    e.stopPropagation();
    startRename(row, folder);
  });
  renameBtn.addEventListener("dblclick", (e) => e.stopPropagation());

  // Wire del pulsante sposta ↗
  // stopPropagation sul click: non deve entrare nella cartella (dblclick check su riga).
  // stopPropagation sul dblclick: per simmetria con gli altri pulsanti riga.
  moveBtn.addEventListener("click", (e) => {
    e.stopPropagation(); // Non far bubblarsi il click alla riga (dblclick→entra)
    openMoveDialog(folder.rel_path, true, folder.name);
  });
  moveBtn.addEventListener("dblclick", (e) => e.stopPropagation());

  // Wire del pulsante elimina (due passi di conferma)
  wireFolderDeleteButton(deleteBtn, row, folder.rel_path);

  return row;
}

// ---------------------------------------------------------------------------
// buildMarkdownItem — costruisce una riga per un documento .md.
//
// Invariato rispetto alle versioni precedenti, con l'aggiunta del parametro
// opzionale onDelete:
//   - null (default): comportamento flat-list (remove() + check empty).
//   - reloadCurrentView: usato nel contesto Explorer per ricaricare la vista.
//
// v0.28.0: aggiunto pulsante ↗ "Sposta" (solo nel contesto Explorer, quando
// onDelete è presente — il tab Find non ha lo spostamento e la sua riga non
// ha il pulsante Sposta).
// ---------------------------------------------------------------------------
function buildMarkdownItem(entry, onDelete = null) {
  const row = document.createElement("div");
  row.className  = "archive-item";
  row.tabIndex   = 0;
  row.setAttribute("role", "option");
  row.dataset.file = entry.file; // rel-path con eventuali sottocartelle

  const titleSpan = document.createElement("span");
  titleSpan.className   = "archive-item-title";
  titleSpan.textContent = entry.title; // textContent — MAI innerHTML

  const dateSpan = document.createElement("span");
  dateSpan.className   = "archive-item-date";
  dateSpan.textContent = formatDate(entry.modified_ms, false);

  const deleteBtn = document.createElement("button");
  deleteBtn.className   = "archive-item-delete";
  deleteBtn.textContent = "\u{1F5D1}"; // 🗑
  deleteBtn.title       = "Elimina";
  deleteBtn.setAttribute("aria-label", "Elimina documento");

  // Passa onDelete come callback: nel contesto Explorer chiama reloadCurrentView
  wireDeleteButton(deleteBtn, row, entry.file, "archive_delete", onDelete);

  row.appendChild(titleSpan);
  row.appendChild(dateSpan);

  // Pulsante ↗ "Sposta" — solo nel contesto Explorer (onDelete != null).
  // Nel tab Find lo spostamento non è disponibile, quindi non mostriamo il pulsante.
  if (onDelete !== null) {
    const moveBtn = document.createElement("button");
    moveBtn.className   = "archive-item-move"; // hover blu (non distruttivo, ≠ rosso del 🗑)
    moveBtn.textContent = "\u{2197}"; // ↗ U+2197 NORTH EAST ARROW
    moveBtn.title       = "Sposta";
    moveBtn.setAttribute("aria-label", "Sposta documento");

    // stopPropagation sul click: non deve aprire il documento (open gestito da row)
    moveBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      openMoveDialog(entry.file, false, entry.title);
    });

    // stopPropagation sul dblclick: il dblclick sulla riga senza guard closest("button")
    // aprirebbe il documento; dobbiamo bloccarlo anche su questo pulsante.
    moveBtn.addEventListener("dblclick", (e) => {
      e.stopPropagation();
    });

    row.appendChild(moveBtn);
  }

  // Pulsante 📤 "Condividi" — solo nel contesto Explorer (onDelete != null),
  // stessa condizione già usata per "Sposta" (il tab Find non condivide).
  if (onDelete !== null) {
    const shareBtn = document.createElement("button");
    shareBtn.className   = "archive-item-move"; // stesso stile hover non-distruttivo del pulsante Sposta
    shareBtn.textContent = "\u{1F4E4}"; // 📤
    shareBtn.title       = "Condividi";
    shareBtn.setAttribute("aria-label", "Condividi documento");

    shareBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      openShareDialog(entry.file, entry.title);
    });
    shareBtn.addEventListener("dblclick", (e) => {
      e.stopPropagation();
    });

    row.appendChild(shareBtn);
  }

  row.appendChild(deleteBtn);

  row.addEventListener("dblclick", () => openMarkdownItem(row.dataset.file));

  row.addEventListener("keydown", (e) => {
    if (e.target !== row) return;
    if (e.key === "Enter") {
      e.preventDefault();
      openMarkdownItem(row.dataset.file);
    }
  });

  return row;
}

// ---------------------------------------------------------------------------
// buildFindItem — costruisce una riga per una sessione Find salvata.
// Invariato rispetto alle versioni precedenti.
// ---------------------------------------------------------------------------
function buildFindItem(entry) {
  const row = document.createElement("div");
  row.className  = "archive-item";
  row.tabIndex   = 0;
  row.setAttribute("role", "option");
  row.dataset.file = entry.file;

  const titleSpan = document.createElement("span");
  titleSpan.className   = "archive-item-title";
  titleSpan.textContent = entry.query; // textContent — MAI innerHTML

  const countSpan = document.createElement("span");
  countSpan.className   = "archive-item-count";
  countSpan.textContent = entry.count + " hit";

  const dateSpan = document.createElement("span");
  dateSpan.className   = "archive-item-date";
  dateSpan.textContent = formatDate(entry.modified, false);

  const deleteBtn = document.createElement("button");
  deleteBtn.className   = "archive-item-delete";
  deleteBtn.textContent = "\u{1F5D1}"; // 🗑
  deleteBtn.title       = "Elimina";
  deleteBtn.setAttribute("aria-label", "Elimina ricerca salvata");

  // Tab Find: nessun onDelete, usa il comportamento originale (remove + check)
  wireDeleteButton(deleteBtn, row, entry.file, "delete_find");

  row.appendChild(titleSpan);
  row.appendChild(countSpan);
  row.appendChild(dateSpan);
  row.appendChild(deleteBtn);

  row.addEventListener("dblclick", () => openFindItem(row.dataset.file));

  row.addEventListener("keydown", (e) => {
    if (e.target !== row) return;
    if (e.key === "Enter") {
      e.preventDefault();
      openFindItem(row.dataset.file);
    }
  });

  return row;
}

// ---------------------------------------------------------------------------
// buildPluginItem — costruisce la riga di un plugin installato (tab Plugins)
// più il suo pannello di dettaglio (il manifest grezzo).
//
// Nessuna azione "open": un solo click sulla riga basta per espandere o
// richiudere il <pre> col contenuto di plugin.json (a differenza degli altri
// tab, qui il dblclick non è usato — click semplice è libero).
//
// Ritorna { row, pre }: il chiamante (loadPluginsTab) decide come/dove
// appenderli entrambi al fragment — qui restano DUE elementi fratelli, non
// annidati, per non alterare il box model della riga (il <pre> non deve
// interferire con click/focus di .archive-item).
// ---------------------------------------------------------------------------
function buildPluginItem(entry) {
  const row = document.createElement("div");
  row.className  = "archive-item";
  row.tabIndex   = 0;
  row.setAttribute("role", "option");
  row.dataset.pluginId = entry.id;

  const titleSpan = document.createElement("span");
  titleSpan.className   = "archive-item-title";
  titleSpan.textContent = entry.name || entry.id; // textContent — MAI innerHTML

  row.appendChild(titleSpan);

  // Pannello di dettaglio: contenuto grezzo del manifest, nascosto finché
  // non si clicca la riga. `manifest_json` è testo letto da file — passa
  // SEMPRE per textContent (mai innerHTML), anche se un plugin.json
  // malevolo dovesse contenere markup.
  const pre = document.createElement("pre");
  pre.className = "plugin-manifest";
  pre.style.display = "none";

  const toggle = () => {
    const isHidden = pre.style.display === "none";
    if (isHidden) pre.textContent = entry.manifest_json;
    pre.style.display = isHidden ? "" : "none";
  };

  row.addEventListener("click", toggle);

  row.addEventListener("keydown", (e) => {
    if (e.target !== row) return;
    if (e.key === "Enter") {
      e.preventDefault();
      toggle();
    }
  });

  return { row, pre };
}

// ---------------------------------------------------------------------------
// buildNoteItem(note) → HTMLElement — costruisce una riga per una nota
// (tab "Note" — Blocco note).
//
// La riga include: titolo, CORPO, meta (macchina — data/ora), ✏️ Modifica,
// 🗑 Elimina.
//
// Corpo (FIX 2 della review finale di branch): fino a quel fix `note.body` era
// plumbato da Rust fino a qui e poi MAI mostrato — la funzione era di sola
// scrittura (si potevano creare e sincronizzare note, ma non leggerne il
// contenuto su nessuna macchina). Il testo arriva già renderizzato
// dall'orchestrator (`render_body` in notes/digest.rs): testo puro, con righe
// `— macchina —` di intestazione quando più macchine hanno contribuito — da cui
// il `white-space: pre-wrap` della classe .note-item-text (v. library.html),
// senza il quale gli a-capo andrebbero persi. Corpo vuoto → nessun elemento
// aggiunto (niente spaziatura fantasma nella riga).
//
// Layout (fix post-review, era un bug del codice letterale del brief): .archive-item
// è flex ORIZZONTALE con justify-content:space-between (per separare [testo] da
// [pulsanti]); titolo e meta vanno però impilati VERTICALMENTE tra loro, quindi
// li avvolgiamo in .note-item-body (flex-direction:column) invece di appenderli
// come figli diretti della riga — altrimenti finirebbero fianco a fianco.
//
// Pulsanti: riusano le classi hover esistenti del file (stesso fix post-review) —
// .archive-item-move (hover blu, azione non distruttiva) per ✏️ e
// .archive-item-delete (hover rosso) per 🗑, invece di restare senza classe con
// il chrome di default del browser.
//
// Eliminazione: due passi di conferma via wireNoteDeleteButton (fix post-review —
// il codice letterale del brief eseguiva al primo click, inconsistente con
// wireDeleteButton/wireFolderDeleteButton usati ovunque altrove in questo file).
//
// row.tabIndex = 0 + role="option" (scostamento minimo dal codice letterale del
// brief, per parità con OGNI altro row builder in questo file — buildFolderRow/
// buildMarkdownItem/buildFindItem/buildPluginItem impostano tutti tabIndex = 0):
// senza, la navigazione da tastiera ArrowUp/Down chiamerebbe .focus() su un <div>
// non focusabile, un no-op silenzioso — tab Note privo di navigazione da tastiera.
//
// Sicurezza: titolo/CORPO/meta (macchina, data) sono dati utente/rete — e il
// corpo può arrivare da un'ALTRA macchina della rete peer → SEMPRE textContent,
// mai innerHTML (invariante di tutto il file, v. il contratto in cima).
// ---------------------------------------------------------------------------
function buildNoteItem(note) {
  const row = document.createElement("div");
  row.className = "archive-item";
  row.tabIndex  = 0;
  row.setAttribute("role", "option");
  row.dataset.noteId = note.id;

  // Sotto-contenitore: impila titolo sopra, meta sotto (v. commento sopra).
  const bodyEl = document.createElement("div");
  bodyEl.className = "note-item-body";

  const titleEl = document.createElement("div");
  titleEl.textContent = note.title || "(senza titolo)";
  bodyEl.appendChild(titleEl);

  // Corpo della nota (FIX 2). textContent, MAI innerHTML: il testo può
  // provenire da un'altra macchina della rete peer. Se il corpo è vuoto non
  // aggiungiamo alcun elemento, per non lasciare spazio vuoto nella riga.
  const bodyText = note.body ?? "";
  if (bodyText !== "") {
    const textEl = document.createElement("div");
    textEl.className   = "note-item-text"; // white-space: pre-wrap (v. library.html)
    textEl.textContent = bodyText;
    bodyEl.appendChild(textEl);
  }

  const metaEl = document.createElement("div");
  metaEl.style.color = "var(--text-muted)";
  metaEl.style.fontSize = "0.85em";
  metaEl.textContent = formatNoteMeta(note.created_by, note.created_at_ms);
  bodyEl.appendChild(metaEl);

  row.appendChild(bodyEl);

  const editBtn = document.createElement("button");
  editBtn.className   = "archive-item-move"; // hover blu, coerente con "Sposta"/"Condividi" (azione non distruttiva)
  editBtn.textContent  = "✏️"; // ✏️
  editBtn.title        = "Modifica";
  editBtn.setAttribute("aria-label", "Modifica nota");
  editBtn.addEventListener("click", (e) => {
    e.stopPropagation();
    // La finestra "Modifica nota" (Tauri separato, v. main.rs open_note_window)
    // è aperta dal main tramite app.js, non da qui direttamente — stesso
    // schema hub di ogni altra finestra secondaria in questo file.
    tauriEvent.emit("library:note-window-open", { note });
  });
  // Simmetria con gli altri pulsanti-riga del file: un doppio-click rapido
  // su ✏️ non deve bubblarsi fino a un eventuale handler dblclick della riga.
  editBtn.addEventListener("dblclick", (e) => e.stopPropagation());
  row.appendChild(editBtn);

  const delBtn = document.createElement("button");
  delBtn.className = "archive-item-delete"; // hover rosso, stesso stile del resto del file
  delBtn.textContent = "\u{1F5D1}"; // 🗑
  delBtn.title = "Elimina";
  delBtn.setAttribute("aria-label", "Elimina nota");
  row.appendChild(delBtn);
  wireNoteDeleteButton(delBtn, note.id);

  return row;
}

// ---------------------------------------------------------------------------
// openMarkdownItem — apre un documento .md dall'archivio.
//
// Il parametro file è ora un rel-path che può contenere "/" (es. "Proj/doc.md").
// archive_open accetta questi path grazie a validate_within_root (Slice 2a).
// ---------------------------------------------------------------------------
async function openMarkdownItem(file) {
  const row = listAreaEl.querySelector(`[data-file="${CSS.escape(file)}"]`);

  let doc;
  try {
    doc = await invokeCmd("archive_open", { file });
  } catch (e) {
    console.error("[library] archive_open failed:", e);
    if (row) flashRowError(row);
    return;
  }

  if (!doc) {
    console.error("[library] archive_open returned null for:", file);
    if (row) flashRowError(row);
    return;
  }

  try {
    await invokeCmd("open_markdown_window", {
      title:      doc.title,
      content:    doc.content,
      kind:       "archived",
      sourceFile: file,
    });
  } catch (e) {
    console.error("[library] open_markdown_window failed:", e);
    if (row) flashRowError(row);
  }
}

// ---------------------------------------------------------------------------
// openFindItem — apre una sessione Find salvata in replay mode.
// Invariato rispetto alle versioni precedenti.
// ---------------------------------------------------------------------------
async function openFindItem(file) {
  const row = listAreaEl.querySelector(`[data-file="${CSS.escape(file)}"]`);

  try {
    await invokeCmd("open_saved_find_window", { file });
  } catch (e) {
    console.error("[library] open_saved_find_window failed:", e);
    if (row) flashRowError(row);
  }
}

// ---------------------------------------------------------------------------
// renderBreadcrumb — aggiorna il DOM del breadcrumb (#lib-breadcrumb).
//
// Costruisce: 🏠 [/] Cartella [/] Sottocartella
// Ogni segmento è un <button> cliccabile che imposta currentPath e chiama
// renderCurrent(). textContent per tutti i nomi (sicurezza XSS).
// ---------------------------------------------------------------------------
function renderBreadcrumb() {
  // Pulisce i figli in modo sicuro (senza innerHTML = "")
  breadcrumbEl.textContent = "";

  // Icona home (🏠) → navigazione alla root
  const homeBtn = document.createElement("button");
  homeBtn.className = "lib-crumb";
  homeBtn.textContent = "\u{1F3E0}"; // 🏠
  homeBtn.title = "Vai alla root della Library";
  homeBtn.setAttribute("aria-label", "Root Library");
  homeBtn.addEventListener("click", () => {
    currentPath = "";
    renderCurrent();
  });
  breadcrumbEl.appendChild(homeBtn);

  // Aggiunge un segmento cliccabile per ogni livello del percorso corrente
  for (const seg of breadcrumbSegments(currentPath)) {
    // Separatore visuale
    const sep = document.createElement("span");
    sep.className   = "lib-crumb-sep";
    sep.textContent = " / ";
    sep.setAttribute("aria-hidden", "true");
    breadcrumbEl.appendChild(sep);

    // Bottone del segmento: textContent = nome (MAI innerHTML)
    const btn = document.createElement("button");
    btn.className   = "lib-crumb";
    btn.textContent = seg.name; // textContent — sicuro
    btn.title       = "Vai a " + seg.name;

    // Chiusura su seg.relPath (valore snapshot al momento della creazione)
    const targetPath = seg.relPath;
    btn.addEventListener("click", () => {
      currentPath = targetPath;
      renderCurrent();
    });

    breadcrumbEl.appendChild(btn);
  }
}

// ---------------------------------------------------------------------------
// renderCurrent — renderizza la cartella corrente (breadcrumb + lista).
//
// Flusso:
// 1. Trova il nodo dell'albero per currentPath.
// 2. Se non trovato (cartella scomparsa): torna alla root.
// 3. Aggiorna il breadcrumb.
// 4. Mostra prima le sottocartelle, poi i documenti.
// 5. Se entrambi vuoti: messaggio "Cartella vuota."
//
// NOTA: NON chiama firstItem.focus() per non rubare il focus a una eventuale
// rinomina inline avviata subito dopo (es. "+ Cartella").
// ---------------------------------------------------------------------------
function renderCurrent() {
  let node = findNode(tree, currentPath);

  if (!node) {
    // La cartella corrente è scomparsa (eliminata dall'esterno o dal nostro delete):
    // torna alla root come fallback sicuro.
    console.warn("[library] currentPath non trovato nell'albero, fallback a root:", currentPath);
    currentPath = "";
    node = findNode(tree, "");
  }

  // Aggiorna il breadcrumb (elemento separato da list-area, sicuro da aggiornare
  // prima di pulire la lista)
  renderBreadcrumb();

  // Pulisce la lista
  listAreaEl.textContent = "";
  listAreaEl.className   = "";

  // Stato vuoto: nessuna sottocartella E nessun documento
  if (node.folders.length === 0 && node.files.length === 0) {
    listAreaEl.className  = "empty";
    listAreaEl.textContent = "Cartella vuota.";
    return;
  }

  // Costruisce il fragment: PRIMA le cartelle, POI i documenti
  const fragment = document.createDocumentFragment();

  for (const folder of node.folders) {
    fragment.appendChild(buildFolderRow(folder));
  }

  for (const file of node.files) {
    // onDelete = reloadCurrentView: dopo il delete ricarica dal backend
    fragment.appendChild(buildMarkdownItem(file, reloadCurrentView));
  }

  listAreaEl.appendChild(fragment);
}

// ---------------------------------------------------------------------------
// reloadCurrentView — ricarica l'albero dal backend e aggiorna la vista.
//
// Esportata logicamente (resa disponibile globalmente sull'oggetto window
// per la Slice 3 fs-watch, che la chiamerà su evento di modifica fs).
// Chiamata da: pulsante 🔄, "+ Cartella", rinomina, elimina.
// ---------------------------------------------------------------------------
async function reloadCurrentView() {
  try {
    tree = await invokeCmd("archive_list_tree");
  } catch (e) {
    console.error("[library] archive_list_tree failed:", e);
    showError("Errore caricamento archivio: " + e);
    return;
  }

  renderCurrent();
}

// Espone reloadCurrentView per la Slice 3 (fs-watch che la chiama via
// window.libraryReload() dall'evento Tauri)
window.libraryReload = reloadCurrentView;

// ---------------------------------------------------------------------------
// loadMarkdownTab — entry point del tab Markdown (vista Explorer).
//
// Prima versione: usava archive_list (lista piatta).
// Adesso: usa archive_list_tree + navigazione a cartelle.
// Delega a reloadCurrentView che fetcha l'albero e chiama renderCurrent().
// ---------------------------------------------------------------------------
async function loadMarkdownTab() {
  await reloadCurrentView();
}

// ---------------------------------------------------------------------------
// loadFindTab — carica e renderizza il tab Find (invariato).
// ---------------------------------------------------------------------------
async function loadFindTab() {
  let entries;
  try {
    entries = await invokeCmd("list_find");
  } catch (e) {
    console.error("[library] list_find failed:", e);
    showError("Errore caricamento ricerche: " + e);
    return;
  }

  if (!Array.isArray(entries) || entries.length === 0) {
    listAreaEl.className  = "empty";
    listAreaEl.textContent = "Nessuna ricerca salvata.";
    return;
  }

  const fragment = document.createDocumentFragment();
  for (const entry of entries) {
    fragment.appendChild(buildFindItem(entry));
  }

  listAreaEl.textContent = "";
  listAreaEl.className   = "";
  listAreaEl.appendChild(fragment);

  const firstItem = listAreaEl.querySelector(".archive-item");
  if (firstItem) firstItem.focus();
}

// ---------------------------------------------------------------------------
// loadPluginsTab — carica e renderizza il tab Plugins (sola lettura).
//
// Ogni entry produce DUE elementi affiancati nel fragment: la riga
// (.archive-item, navigabile con ArrowUp/Down) e il suo <pre> di dettaglio
// (nascosto finché non si clicca la riga). Il <pre> NON ha class
// "archive-item" apposta: se ce l'avesse, la navigazione da tastiera
// generica (che interroga #list-area .archive-item) lo tratterebbe come
// una riga selezionabile.
// ---------------------------------------------------------------------------
async function loadPluginsTab() {
  let entries;
  try {
    entries = await invokeCmd("list_plugins");
  } catch (e) {
    console.error("[library] list_plugins failed:", e);
    showError("Errore caricamento plugin: " + e);
    return;
  }

  if (!Array.isArray(entries) || entries.length === 0) {
    listAreaEl.className  = "empty";
    listAreaEl.textContent = "Nessun plugin installato.";
    return;
  }

  const fragment = document.createDocumentFragment();
  for (const entry of entries) {
    const { row, pre } = buildPluginItem(entry);
    fragment.appendChild(row);
    fragment.appendChild(pre);
  }

  listAreaEl.textContent = "";
  listAreaEl.className   = "";
  listAreaEl.appendChild(fragment);

  const firstItem = listAreaEl.querySelector(".archive-item");
  if (firstItem) firstItem.focus();
}

// ---------------------------------------------------------------------------
// loadNoteTab — entry point del tab Note (Blocco note).
//
// A differenza di Markdown/Find/Plugins (che PULLano via invoke), le note
// arrivano via WS/eventi Tauri (library:notes-snapshot, spinto da app.js).
// Qui: (1) renderizziamo subito quello che già abbiamo in currentNotes, (2)
// chiediamo un refresh esplicito (nel caso lo snapshot sia arrivato prima che
// questa finestra si aprisse), (3) leggiamo lo stato `enabled` della rete peer
// e ri-renderizziamo, così una lista vuota può dire il VERO motivo per cui è
// vuota (FIX 6, v. `peerNetworkEnabled`).
//
// `async` senza await lato chiamante: `activateTab` non aspetta: il primo
// render è già avvenuto in modo sincrono, il secondo arriva quando la lettura
// del config si risolve (millisecondi, e comunque non blocca il tab).
// ---------------------------------------------------------------------------
async function loadNoteTab() {
  renderNoteList();
  tauriEvent?.emit?.("library:request-notes", {});

  try {
    const settings = await invokeCmd("get_aichat_settings");
    // Si assegna SOLO davanti a un vero booleano. Se il comando non è
    // disponibile (finestra aperta fuori da Tauri) `invokeCmd` ritorna
    // `undefined`, e un `settings.enabled` mancante darebbe `undefined`:
    // in entrambi i casi vogliamo restare in "non so" (null), non scivolare
    // in "disabilitata" — che accuserebbe l'utente di una configurazione che
    // non abbiamo letto.
    // Non sovrascrivere un `true` già confermato da uno snapshot live (v.
    // sotto, listener "library:notes-snapshot"): questa lettura del config e
    // quel listener corrono in parallelo (`loadNoteTab` non aspetta questa
    // Promise prima di ritornare), e se il config legge `enabled: false`
    // MENTRE il servizio è di fatto già attivo (es. l'utente ha appena
    // cambiato il file ma non ha ancora riavviato l'orchestrator), arrivare
    // dopo lo snapshot non deve smentirlo — lo snapshot è la prova più forte
    // (solo AiChatService lo emette, solo se è davvero acceso), il file dice
    // solo com'è CONFIGURATO. Senza questa guardia l'ordine di arrivo delle
    // due Promise decide arbitrariamente chi vince.
    if (settings && typeof settings.enabled === "boolean" && peerNetworkEnabled !== true) {
      peerNetworkEnabled = settings.enabled;
    }
  } catch (e) {
    console.error("[library] get_aichat_settings error:", e);
    // peerNetworkEnabled resta null: nessuna affermazione.
  }
  if (activeTab === "note") renderNoteList();
}

// ---------------------------------------------------------------------------
// renderNoteList — renderizza currentNotes (filtrato/ordinato via
// visibleNotesSorted) in #list-area.
//
// Lista vuota (FIX 6): il messaggio dipende dal PERCHÉ è vuota. Con la rete
// peer spenta il Blocco note non può funzionare per niente (nessun servizio,
// nessuno snapshot, e "Nuova" produrrebbe un messaggio che nessuno ascolta) —
// lo si dice esplicitamente, indicando il file, il campo e dove si cambia,
// nello stesso tono diretto della nota del tab /config → AI Chat. Il pulsante
// "Nuova" viene disabilitato nello stesso caso: meglio un pulsante
// visibilmente spento che uno che accetta il click e non fa nulla.
// ---------------------------------------------------------------------------
function renderNoteList() {
  // Il pulsante è disabilitato SOLO su un "disabilitata" accertato: in stato
  // sconosciuto (null) resta attivo, per non bloccare un utente su una lettura
  // di config fallita.
  if (newNoteBtn) newNoteBtn.disabled = peerNetworkEnabled === false;

  const notes = visibleNotesSorted(currentNotes);
  if (notes.length === 0) {
    listAreaEl.className  = "empty";
    listAreaEl.textContent = peerNetworkEnabled === false
      ? "Rete peer disabilitata (network.json, campo \"enabled\") — abilitala da /config → AI Chat per usare il Blocco note. Ha effetto al prossimo riavvio dell'orchestrator."
      : "Nessuna nota.";
    return;
  }
  const fragment = document.createDocumentFragment();
  for (const note of notes) {
    fragment.appendChild(buildNoteItem(note));
  }
  listAreaEl.textContent = "";
  listAreaEl.className   = "";
  listAreaEl.appendChild(fragment);
}

// ---------------------------------------------------------------------------
// Tab switching: aggiorna CSS/ARIA, mostra/nasconde toolbar, ricarica lista.
// ---------------------------------------------------------------------------
function activateTab(tab) {
  activeTab = tab;

  [tabMarkdownEl, tabFindEl, tabPluginsEl, tabNoteEl].forEach((btn) => {
    const isActive = btn.dataset.tab === tab;
    btn.classList.toggle("lib-tab--active", isActive);
    btn.setAttribute("aria-selected", isActive ? "true" : "false");
  });

  // Mostra la toolbar Explorer solo sul tab Markdown, la toolbar Note solo
  // sul tab Note (sul tab Find/Plugins nessuna delle due serve).
  if (toolbarEl)     toolbarEl.style.display     = tab === "markdown" ? "" : "none";
  if (noteToolbarEl) noteToolbarEl.style.display = tab === "note" ? "" : "none";

  listAreaEl.textContent = "";
  listAreaEl.className   = "";

  if (tab === "find") {
    loadFindTab();
  } else if (tab === "plugins") {
    loadPluginsTab();
  } else if (tab === "note") {
    loadNoteTab();
  } else {
    loadMarkdownTab();
  }
}

tabMarkdownEl.addEventListener("click", () => {
  if (activeTab !== "markdown") activateTab("markdown");
});

tabFindEl.addEventListener("click", () => {
  if (activeTab !== "find") activateTab("find");
});

tabPluginsEl.addEventListener("click", () => {
  if (activeTab !== "plugins") activateTab("plugins");
});

tabNoteEl.addEventListener("click", () => {
  if (activeTab !== "note") activateTab("note");
});

// Attivazione tab via tastiera (Enter/Space — pattern ARIA tablist)
[tabMarkdownEl, tabFindEl, tabPluginsEl, tabNoteEl].forEach((btn) => {
  btn.addEventListener("keydown", (e) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      btn.click();
    }
  });
});

// ---------------------------------------------------------------------------
// Toolbar: pulsante "+ Cartella"
//
// Flusso:
// 1. Invoca archive_create_folder con nome "Nuova cartella".
//    Il backend gestisce le collisioni → potrebbe restituire "Nuova cartella 2".
// 2. Ricarica la vista (l'albero include la nuova cartella).
// 3. Trova la riga col rel_path restituito dal backend (NON il nome letterale).
// 4. Avvia la rinomina inline su quella riga.
//
// Il passo 3 usa il rel_path del backend, non "Nuova cartella", perché
// su collisione il backend potrebbe aver generato "Nuova cartella 2".
// ---------------------------------------------------------------------------
if (newFolderBtn) {
  newFolderBtn.addEventListener("click", async () => {
    try {
      // archive_create_folder ritorna il rel_path effettivo della cartella creata
      const newRel = await invokeCmd("archive_create_folder", {
        parentRel: currentPath,
        name: "Nuova cartella",
      });

      if (!newRel) {
        console.error("[library] archive_create_folder returned null");
        return;
      }

      // Ricarica la vista: ora l'albero include la cartella appena creata
      await reloadCurrentView();

      // Trova la riga della nuova cartella per il rel_path restituito dal backend
      const newRow = listAreaEl.querySelector(
        `[data-folder-rel="${CSS.escape(newRel)}"]`
      );

      if (newRow && newRow._folderData) {
        // Avvia la rinomina inline: l'utente vede l'input già selezionato
        startRename(newRow, newRow._folderData);
      } else {
        console.warn("[library] riga nuova cartella non trovata per rel:", newRel);
      }
    } catch (e) {
      console.error("[library] archive_create_folder failed:", e);
    }
  });
}

// Pulsante 🔄 Ricarica
if (reloadBtn) {
  reloadBtn.addEventListener("click", () => {
    reloadCurrentView();
  });
}

// ---------------------------------------------------------------------------
// Toolbar Note: pulsante "Nuova" — chiede al main di aprire la finestra
// "Nuova nota" (note = null/assente, v. main.rs open_note_window).
// ---------------------------------------------------------------------------
if (newNoteBtn) {
  newNoteBtn.addEventListener("click", () => {
    tauriEvent.emit("library:note-window-open", { note: null });
  });
}

// ---------------------------------------------------------------------------
// Bootstrap: inizializza la finestra sul tab Markdown (default).
// ---------------------------------------------------------------------------
async function bootstrap() {
  if (!tauriInvoke) {
    listAreaEl.className  = "error";
    listAreaEl.textContent = "[library] Tauri IPC non disponibile.";
    return;
  }

  // Trasparenza (--window-alpha): applica subito il valore corrente della
  // config all'avvio, come fa app.js per la finestra principale. Legge
  // get_config invece di aspettare un evento (questa finestra potrebbe
  // aprirsi molto dopo l'ultimo salvataggio di /config).
  try {
    const cfg = await invokeCmd("get_config");
    if (cfg && typeof cfg.window_alpha === "number") {
      document.documentElement.style.setProperty("--window-alpha", cfg.window_alpha);
    }
  } catch (e) {
    console.warn("[library] get_config on startup failed:", e);
  }

  activateTab("markdown");

  // Collega i handler della modale "Sposta" (Slice 4b) — una sola volta a bootstrap.
  // wireMoveDialog() registra i listener su #move-confirm e #move-cancel una sola
  // volta; ad ogni apertura, openMoveDialog() aggiorna solo i contenuti del dialog.
  wireMoveDialog();

  // Collega i handler della modale "Condividi" (Slice 1a-ui) — stesso principio
  // di wireMoveDialog: una sola registrazione, letta da shareState al click.
  wireShareDialog();

  // fs-watch (Slice 3): il backend Rust osserva la cartella Library con
  // notify-debouncer-mini e, quando rileva modifiche (create/rename/delete),
  // emette l'evento "library:changed" dopo un debounce di 400 ms.
  // Qui ci iscriviamo e ricarichiamo la vista — ma solo se siamo sul tab
  // Markdown (il tab Find non mostra l'albero delle cartelle).
  //
  // Guardia anti-clobber: notify segnala TUTTE le modifiche al filesystem,
  // comprese quelle generate dalla nostra stessa UI (es. archive_create_folder
  // → crea la dir → scatta il watcher dopo 400 ms).
  // Se in quel momento c'è una rinomina inline attiva (.lib-rename-input),
  // il reload distruggerebbe l'<input> e resetterebbe il nome al valore
  // originale mentre l'utente sta ancora digitando.
  // Soluzione: saltiamo il reload se il focus è su un <input> qualsiasi
  // (coprono: rinomina cartella, ma anche eventuali futuri input inline).
  if (tauriEvent?.listen) {
    tauriEvent.listen("library:changed", () => {
      if (activeTab !== "markdown") return;
      // Salta il reload se c'è una rinomina inline attiva: il reload distruggerebbe
      // l'<input> mentre l'utente sta digitando il nuovo nome.
      if (document.activeElement?.tagName === "INPUT") return;
      // Salta il reload se la modale "Sposta" è aperta: ridisegnare la lista sotto
      // il dialog aperto creerebbe inconsistenza visiva e perderebbe la selezione.
      if (moveDialog?.open) return;
      reloadCurrentView();
    });

    // Trasparenza (--window-alpha): riapplica ad ogni salvataggio di /config,
    // anche se questa finestra è già aperta (broadcast globale Tauri).
    tauriEvent.listen("config:saved", (ev) => {
      const alpha = ev.payload?.window_alpha;
      if (typeof alpha === "number") {
        document.documentElement.style.setProperty("--window-alpha", alpha);
      }
    });

    // Roster AI Chat (Slice 1a-ui): aggiornamento live per il dialog "Condividi".
    tauriEvent.listen("library:roster", (e) => {
      currentRoster = e.payload?.participants ?? [];
    });

    // Raggiungibilità di rete per "Condividi" (fix 2026-07-28): canale
    // separato dal roster di ammissione chat sopra.
    tauriEvent.listen("library:reachable-peers", (e) => {
      currentReachablePeers = e.payload?.labels ?? [];
    });

    // Blocco note (Task 16/19): snapshot completo (replay su richiesta/riconnessione
    // — v. loadNoteTab) e push incrementale per singola nota creata/modificata.
    // Aggiorna currentNotes e ri-renderizza SOLO se il tab Note è quello attivo
    // (altrimenti la prossima loadNoteTab userà comunque lo stato aggiornato).
    tauriEvent.listen("library:notes-snapshot", (e) => {
      currentNotes = e.payload?.notes ?? [];
      // Uno snapshot che ARRIVA è la prova più forte possibile che il servizio
      // è vivo: solo `AiChatService` lo emette, e parte solo con `enabled:
      // true`. Vale più della lettura del file di config fatta in loadNoteTab
      // (che dice com'è CONFIGURATO, non se è davvero SU), e soprattutto
      // aggiorna lo stato senza aspettare un cambio di scheda — altrimenti il
      // pulsante "Nuova" resterebbe disabilitato, senza motivo visibile,
      // finché l'utente non esce dal tab e ci rientra (FIX 6).
      peerNetworkEnabled = true;
      if (activeTab === "note") renderNoteList();
    });

    tauriEvent.listen("library:note-upserted", (e) => {
      const note = e.payload?.note;
      if (!note) return;
      const idx = currentNotes.findIndex((n) => n.id === note.id);
      if (idx >= 0) currentNotes[idx] = note; else currentNotes.push(note);
      if (activeTab === "note") renderNoteList();
    });

    // Richiede subito il roster corrente — se un AiChatRoster è già arrivato ad
    // app.js prima che questa finestra si aprisse, altrimenti currentRoster
    // resterebbe vuoto finché non arriva un aggiornamento live successivo.
    if (tauriEvent?.emit) {
      tauriEvent.emit("library:request-roster", {});
      tauriEvent.emit("library:request-reachable-peers", {});
    }
  }
}

bootstrap();
