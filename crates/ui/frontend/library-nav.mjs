/**
 * library-nav.mjs — Modulo puro per la navigazione Explorer della Library.
 *
 * Nessuna dipendenza da DOM, Tauri o I/O: tutte le funzioni sono pure,
 * deterministe e testabili con `node:test` senza ambiente browser.
 *
 * Concetto chiave: il frontend carica l'albero completo UNA volta con
 * `archive_list_tree`, poi naviga in memoria modificando `currentPath`.
 * Questo modulo fornisce le operazioni pure su quell'albero.
 *
 * Tipi usati (schema Rust serializzato via Serde):
 *
 *   LibraryTree {
 *     root_files: ArchiveEntry[],   // file .md alla root della library
 *     folders:    LibraryNode[],    // sottocartelle di primo livello
 *   }
 *
 *   LibraryNode {
 *     name:     string,             // nome della cartella (es. "Progetti")
 *     rel_path: string,             // path relativo alla root (es. "Progetti/Web")
 *     folders:  LibraryNode[],      // sottocartelle ricorsive
 *     files:    ArchiveEntry[],     // file .md in questa cartella
 *   }
 *
 *   ArchiveEntry {
 *     title:       string,
 *     file:        string,          // rel-path completo (es. "Progetti/Web/doc.md")
 *     modified_ms: number,          // timestamp ms
 *   }
 *
 * relPath: percorso relativo alla root della Library, "/" come separatore.
 *          "" = root, "Progetti" = primo livello, "Progetti/Web" = annidato.
 */

// ---------------------------------------------------------------------------
// isValidFolderName(name) → boolean
//
// Valida un nome di cartella da creare o rinominare.
//
// Restituisce FALSE se:
//   - la stringa è vuota o composta di soli spazi
//   - contiene "/" (separatore di path — renderebbe il nome ambiguo)
//   - contiene "\" (backslash — illegale nei nomi file su Windows)
//   - contiene ".." (sottostringa — potenziale path traversal)
//
// Restituisce TRUE per nomi legittimi come "Progetti 2024", "Web", "Note".
//
// Nota didattica: questa validazione è client-side per UX immediata.
// Il backend (validate_within_root) ricontrolla lato server: due livelli
// di difesa indipendenti (principio di difesa in profondità).
// ---------------------------------------------------------------------------
export function isValidFolderName(name) {
  // Rifiuta tipi non-stringa e stringhe vuote o di soli spazi
  if (typeof name !== "string" || name.trim() === "") return false;

  // Rifiuta caratteri che renderebbero il path ambiguo o pericoloso
  if (name.includes("/") || name.includes("\\") || name.includes("..")) {
    return false;
  }

  return true;
}

// ---------------------------------------------------------------------------
// isFolderEmpty(node) → boolean
//
// Controlla ottimisticamente se un LibraryNode è vuoto (nessuna sottocartella,
// nessun file). Usato SOLO per mostrare/nascondere il pulsante 🗑 nella UI.
//
// ⚠ ATTENZIONE: questa è una stima lato client basata sull'albero già caricato.
// La fonte di verità è il backend: `archive_delete_folder` rifiuta se la
// cartella contiene file non visibili nella UI (es. file non-.md).
// In caso di Err dal backend → mostrare flashRowError + reloadCurrentView.
// ---------------------------------------------------------------------------
export function isFolderEmpty(node) {
  return node.folders.length === 0 && node.files.length === 0;
}

// ---------------------------------------------------------------------------
// findNode(tree, relPath) → { folders, files } | null
//
// Naviga l'albero LibraryTree fino alla cartella identificata da relPath.
//
// Casi:
//   relPath === "" → root → { folders: tree.folders, files: tree.root_files }
//   relPath === "A" → cerca nodo con name="A" tra tree.folders
//   relPath === "A/B" → cerca "A", poi cerca "B" tra A.folders
//
// Restituisce null se il path non esiste nell'albero corrente (es. cartella
// eliminata dall'esterno tra un reload e l'altro).
//
// Nota didattica: siccome `isValidFolderName` proibisce "/" nei nomi,
// ogni segmento del split corrisponde esattamente a un nome di cartella.
// Non c'è ambiguità tra separatore e carattere nel nome.
// ---------------------------------------------------------------------------
export function findNode(tree, relPath) {
  // Caso root: restituisce i dati di primo livello direttamente da tree
  if (relPath === "") {
    return { folders: tree.folders, files: tree.root_files };
  }

  // Cammina l'albero seguendo ogni segmento del path
  const segments = relPath.split("/");
  let currentFolders = tree.folders; // Livello corrente di ricerca
  let currentNode = null;            // Nodo trovato a ogni step

  for (const seg of segments) {
    // Cerca il nodo con name === segmento corrente
    currentNode = currentFolders.find((f) => f.name === seg);
    if (!currentNode) return null; // Segmento mancante → path inesistente

    // Scendi al livello successivo per il prossimo segmento
    currentFolders = currentNode.folders;
  }

  // Restituisce folders e files della cartella terminale
  return { folders: currentNode.folders, files: currentNode.files };
}

// ---------------------------------------------------------------------------
// breadcrumbSegments(relPath) → Array<{ name: string, relPath: string }>
//
// Converte un relPath in un array di segmenti per il breadcrumb cliccabile.
//
// Esempi:
//   ""        → []
//   "A"       → [{ name: "A", relPath: "A" }]
//   "A/B"     → [{ name: "A", relPath: "A" }, { name: "B", relPath: "A/B" }]
//   "A/B/C"   → [{ name: "A", relPath: "A" }, { name: "B", relPath: "A/B" },
//                { name: "C", relPath: "A/B/C" }]
//
// Il campo `relPath` di ogni segmento è CUMULATIVO: è il percorso completo
// per navigare fino a quel livello (usato dal click handler del breadcrumb).
//
// Nota didattica: `parts.slice(0, i + 1).join("/")` costruisce il relPath
// incrementale senza allocazioni temporanee eccessive (slice crea un nuovo
// array, ma è O(n) per livello — accettabile per alberi piccoli).
// ---------------------------------------------------------------------------
export function breadcrumbSegments(relPath) {
  if (relPath === "") return [];

  const parts = relPath.split("/");
  return parts.map((name, i) => ({
    name,
    // Il relPath cumulativo fino a questo indice (incluso)
    relPath: parts.slice(0, i + 1).join("/"),
  }));
}

// ---------------------------------------------------------------------------
// parentPath(relPath) → string
//
// Calcola il relPath della cartella padre.
//
// Esempi:
//   ""        → ""   (la root non ha parent)
//   "A"       → ""   (il parent di un singolo segmento è la root)
//   "A/B"     → "A"
//   "A/B/C"   → "A/B"
//
// Usato da `renderCurrent` come fallback quando una cartella scompare
// (es. eliminata dall'esterno): risale al padre prima di ricaricare.
// ---------------------------------------------------------------------------
export function parentPath(relPath) {
  // Root o singolo segmento: il parent è la root (stringa vuota)
  if (relPath === "" || !relPath.includes("/")) return "";

  // Taglia tutto dal "/" finale in poi
  return relPath.slice(0, relPath.lastIndexOf("/"));
}

// ---------------------------------------------------------------------------
// flattenFolders(tree) → Array<{ name, relPath, depth }>
//
// Visita l'albero LibraryTree in pre-order e restituisce UN array piatto con
// TUTTE le cartelle.
//
// - name:    il nome della cartella (es. "Progetti")
// - relPath: il path relativo già presente nel nodo (es. "Progetti/Web")
// - depth:   profondità assoluta di annidatura, 0 = primo livello sotto la radice
//
// Pre-order: ogni cartella padre precede i suoi figli → l'ordine riflette la
// navigazione naturale top-down dell'albero.
//
// Esempio:
//   albero: A/ → A/B/ → A/B/C/, D/
//   risultato: [A(0), A/B(1), A/B/C(2), D(0)]
//
// Nota: la funzione è pura (nessun side effect) — testabile senza DOM/Tauri.
// ---------------------------------------------------------------------------
export function flattenFolders(tree) {
  const result = [];

  // Visita ricorsiva in pre-order.
  // `folders`: array di LibraryNode al livello corrente.
  // `depth`:   profondità corrente (parte da 0 per i figli diretti di root).
  function visit(folders, depth) {
    for (const node of folders) {
      // Aggiunge il nodo corrente PRIMA dei suoi figli (pre-order)
      result.push({ name: node.name, relPath: node.rel_path, depth });
      // Scende ricorsivamente nei figli con depth + 1
      visit(node.folders, depth + 1);
    }
  }

  // Inizia dalla lista di cartelle di primo livello (depth 0)
  visit(tree.folders, 0);
  return result;
}

// ---------------------------------------------------------------------------
// moveTargets(tree, itemRelPath, isFolder) → Array<{ name, relPath, depth }>
//
// Calcola l'elenco delle destinazioni VALIDE per spostare l'elemento
// identificato da `itemRelPath`.
//
// Regole di filtro:
//   1. Parte da flattenFolders(tree) — tutte le cartelle dell'albero.
//   2. Esclude il parent corrente (parentPath(itemRelPath)):
//      spostare un elemento dove si trova già è un no-op / collisione.
//   3. Se isFolder (l'elemento da spostare è una cartella):
//      - Esclude la cartella stessa (relPath === itemRelPath).
//      - Esclude ogni suo discendente (relPath che inizia con itemRelPath + "/").
//      NOTA: il "/" finale è cruciale per non escludere cartelle con nome che
//      inizia per coincidenza con itemRelPath (es. "AB" non è discendente di "A").
//   4. Antepone l'opzione ROOT { name:"Documenti (radice)", relPath:"", depth:0 }
//      TRANNE quando il parent corrente è già la root (parentPath === ""),
//      perché l'elemento è già al primo livello e spostarlo alla root sarebbe
//      di nuovo un no-op / collisione.
//
// Ogni riga del risultato porta anche `disabled`: true SOLO per la riga che
// coincide col parent corrente (regola 2). Il parent NON viene più rimosso
// dalla lista — resta come riga non selezionabile, per un motivo preciso:
//
// BUG CORRETTO: rimuovendo del tutto il parent, i suoi altri figli (che
// restano in lista alla loro depth originale, es. depth 1) perdevano
// l'ancora visiva del vero genitore. L'indentazione-only della UI (nessuna
// linea di connessione, solo padding-left proporzionale a depth) li faceva
// SEMBRARE annidati sotto qualunque riga precedente a depth minore — anche
// se non aveva nessuna relazione reale (bug osservato dal vivo: una
// sottocartella appena creata sembrava dentro la cartella sbagliata perché
// la sua vera madre, esclusa come "parent corrente", non compariva più).
// Tenendo il parent in lista (disabilitato) l'indentazione dei fratelli
// rimasti punta di nuovo alla riga giusta.
//
// Esempi (albero A→A/B→A/B/C, sibling D):
//   moveTargets(tree, "A/note.md", false) → [root, A(disabled), A/B, A/B/C, D]
//   moveTargets(tree, "A", true)          → [D]  (A è la sorgente stessa: esclusa del tutto, non un'ancora)
//   moveTargets(tree, "A/B", true)        → [root, A(disabled), D]  (A/B=self escluso, A/B/C=discendente escluso)
//
// Ritorna [] se non esistono destinazioni valide → la UI mostra "Nessuna disponibile".
// ---------------------------------------------------------------------------
export function moveTargets(tree, itemRelPath, isFolder) {
  const allFolders = flattenFolders(tree);
  const parent     = parentPath(itemRelPath);

  // Filtra: SOLO la sorgente stessa e i suoi discendenti vengono rimossi
  // del tutto (non hanno senso come destinazione). Il parent corrente NON
  // viene filtrato qui — resta, e viene marcato `disabled` sotto.
  const filtered = allFolders.filter(({ relPath }) => {
    if (!isFolder) return true;

    // Regola 3a: escludi la cartella stessa
    if (relPath === itemRelPath) return false;
    // Regola 3b: escludi i discendenti (prefisso + "/" per evitare falsi positivi)
    if (relPath.startsWith(itemRelPath + "/")) return false;

    return true;
  });

  // Regola 2 (rivista): marca come disabled la riga che coincide col parent
  // corrente, invece di rimuoverla — resta come ancora visiva non cliccabile.
  const withDisabled = filtered.map((f) => ({
    ...f,
    disabled: f.relPath === parent,
  }));

  // Regola 4: anteponi la root solo se il parent NON è già la root
  if (parent !== "") {
    return [{ name: "Documenti (radice)", relPath: "", depth: 0, disabled: false }, ...withDisabled];
  }

  return withDisabled;
}
