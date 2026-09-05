// library-nav.test.mjs — Test TDD per il modulo puro library-nav.mjs.
//
// Queste sono le specifiche eseguibili del comportamento di navigazione
// Explorer della Library. Ogni test descrive UN comportamento preciso.
// Leggi questi test prima di leggere l'implementazione: sono la fonte di
// verità su cosa fanno le funzioni pure.
//
// Esegui con:  node --test crates/ui/frontend/library-nav.test.mjs

import { test } from "node:test";
import assert from "node:assert/strict";
import {
  isValidFolderName,
  isFolderEmpty,
  findNode,
  breadcrumbSegments,
  parentPath,
  flattenFolders,
  moveTargets,
} from "./library-nav.mjs";

// ===========================================================================
// isValidFolderName — validazione nomi di cartella
// ===========================================================================

test("isValidFolderName: stringa vuota → false", () => {
  assert.equal(isValidFolderName(""), false);
});

test("isValidFolderName: solo spazi → false", () => {
  assert.equal(isValidFolderName("   "), false);
});

test("isValidFolderName: contiene '/' → false", () => {
  assert.equal(isValidFolderName("a/b"), false);
});

test("isValidFolderName: contiene backslash → false", () => {
  assert.equal(isValidFolderName("a\\b"), false);
});

test("isValidFolderName: '..' esatto → false", () => {
  assert.equal(isValidFolderName(".."), false);
});

test("isValidFolderName: contiene '..' come sottostringa → false", () => {
  // Anche nomi come "a..b" sono rifiutati per sicurezza
  assert.equal(isValidFolderName("a..b"), false);
});

test("isValidFolderName: nome semplice → true", () => {
  assert.equal(isValidFolderName("Progetti"), true);
});

test("isValidFolderName: nome con spazi interni → true", () => {
  // Spazi interni sono consentiti (solo spazi come unico contenuto è false)
  assert.equal(isValidFolderName("Nuova cartella"), true);
});

test("isValidFolderName: nome con numeri → true", () => {
  assert.equal(isValidFolderName("Progetto 2024"), true);
});

// ===========================================================================
// isFolderEmpty — stima ottimistica del vuoto (per mostrare/nascondere 🗑)
// ===========================================================================

test("isFolderEmpty: cartella senza sottocartelle e senza file → true", () => {
  assert.equal(isFolderEmpty({ folders: [], files: [] }), true);
});

test("isFolderEmpty: ha sottocartelle → false", () => {
  assert.equal(isFolderEmpty({ folders: [{ name: "sub" }], files: [] }), false);
});

test("isFolderEmpty: ha file → false", () => {
  assert.equal(
    isFolderEmpty({ folders: [], files: [{ file: "doc.md" }] }),
    false,
  );
});

test("isFolderEmpty: ha sia cartelle che file → false", () => {
  assert.equal(
    isFolderEmpty({
      folders: [{ name: "sub" }],
      files: [{ file: "doc.md" }],
    }),
    false,
  );
});

// ===========================================================================
// findNode — navigazione dell'albero LibraryTree
//
// Struttura dell'albero di test:
//   root_files: [ { title: "Radice", file: "radice.md" } ]
//   folders:
//     - Progetti/
//       - Web/            (ha 1 file: "App")
//       note.md           (file in Progetti/)
//     - Vuota/            (vuota)
// ===========================================================================

const sampleTree = {
  root_files: [{ title: "Radice", file: "radice.md", modified_ms: 1000 }],
  folders: [
    {
      name: "Progetti",
      rel_path: "Progetti",
      folders: [
        {
          name: "Web",
          rel_path: "Progetti/Web",
          folders: [],
          files: [
            { title: "App", file: "Progetti/Web/app.md", modified_ms: 2000 },
          ],
        },
      ],
      files: [
        { title: "Note", file: "Progetti/note.md", modified_ms: 1500 },
      ],
    },
    {
      name: "Vuota",
      rel_path: "Vuota",
      folders: [],
      files: [],
    },
  ],
};

test("findNode: path vuoto → root con root_files e folders di primo livello", () => {
  const result = findNode(sampleTree, "");
  assert.equal(result.files.length, 1);
  assert.equal(result.files[0].title, "Radice");
  assert.equal(result.folders.length, 2);
  assert.equal(result.folders[0].name, "Progetti");
});

test("findNode: un livello → nodo Progetti con file e sottocartelle", () => {
  const result = findNode(sampleTree, "Progetti");
  assert.equal(result.files.length, 1);
  assert.equal(result.files[0].title, "Note");
  assert.equal(result.folders.length, 1);
  assert.equal(result.folders[0].name, "Web");
});

test("findNode: due livelli → nodo annidato Progetti/Web", () => {
  const result = findNode(sampleTree, "Progetti/Web");
  assert.equal(result.files.length, 1);
  assert.equal(result.files[0].title, "App");
  assert.equal(result.folders.length, 0);
});

test("findNode: path inesistente al primo livello → null", () => {
  assert.equal(findNode(sampleTree, "NonEsiste"), null);
});

test("findNode: path parzialmente inesistente → null", () => {
  // "Progetti" esiste ma "Progetti/NonEsiste" no
  assert.equal(findNode(sampleTree, "Progetti/NonEsiste"), null);
});

test("findNode: cartella vuota → { folders: [], files: [] }", () => {
  const result = findNode(sampleTree, "Vuota");
  assert.equal(result.folders.length, 0);
  assert.equal(result.files.length, 0);
});

// ===========================================================================
// breadcrumbSegments — segmenti cliccabili del breadcrumb
// ===========================================================================

test("breadcrumbSegments: path vuoto → array vuoto", () => {
  assert.deepEqual(breadcrumbSegments(""), []);
});

test("breadcrumbSegments: un livello → un segmento con relPath uguale al nome", () => {
  assert.deepEqual(breadcrumbSegments("Progetti"), [
    { name: "Progetti", relPath: "Progetti" },
  ]);
});

test("breadcrumbSegments: due livelli → relPath cumulativo per ogni segmento", () => {
  assert.deepEqual(breadcrumbSegments("Progetti/Web"), [
    { name: "Progetti", relPath: "Progetti" },
    { name: "Web",      relPath: "Progetti/Web" },
  ]);
});

test("breadcrumbSegments: tre livelli → tre segmenti con accumulo corretto", () => {
  assert.deepEqual(breadcrumbSegments("A/B/C"), [
    { name: "A", relPath: "A" },
    { name: "B", relPath: "A/B" },
    { name: "C", relPath: "A/B/C" },
  ]);
});

// ===========================================================================
// parentPath — path della cartella padre
// ===========================================================================

test("parentPath: path vuoto (root) → '' (la root non ha parent)", () => {
  assert.equal(parentPath(""), "");
});

test("parentPath: singolo segmento → '' (il parent è la root)", () => {
  assert.equal(parentPath("Progetti"), "");
});

test("parentPath: due livelli → primo livello", () => {
  assert.equal(parentPath("Progetti/Web"), "Progetti");
});

test("parentPath: tre livelli → due livelli", () => {
  assert.equal(parentPath("A/B/C"), "A/B");
});

// ===========================================================================
// flattenFolders — lista piatta di tutte le cartelle in pre-order
//
// Struttura dell'albero usato per i test di flattenFolders e moveTargets:
//
//   root_files: [ doc.md ]
//   folders:
//     - A/                   (depth 0)
//       - A/B/               (depth 1)
//         - A/B/C/           (depth 2)
//       files: A/note.md
//     - D/                   (depth 0)
//
// Questo albero copre: pre-order, depth variabile, sibling, foglia, radice.
// ===========================================================================

const moveTree = {
  root_files: [
    { title: "Root doc", file: "doc.md", modified_ms: 1000 },
  ],
  folders: [
    {
      name: "A",
      rel_path: "A",
      folders: [
        {
          name: "B",
          rel_path: "A/B",
          folders: [
            {
              name: "C",
              rel_path: "A/B/C",
              folders: [],
              files: [],
            },
          ],
          files: [],
        },
      ],
      files: [
        { title: "Note", file: "A/note.md", modified_ms: 1500 },
      ],
    },
    {
      name: "D",
      rel_path: "D",
      folders: [],
      files: [],
    },
  ],
};

test("flattenFolders: albero vuoto → array vuoto", () => {
  // Caso base: nessuna cartella definita
  const emptyTree = { root_files: [], folders: [] };
  assert.deepEqual(flattenFolders(emptyTree), []);
});

test("flattenFolders: albero con cartelle piatto → depth 0 per tutti", () => {
  // Due cartelle di primo livello, nessuna annidatura
  const flat = {
    root_files: [],
    folders: [
      { name: "X", rel_path: "X", folders: [], files: [] },
      { name: "Y", rel_path: "Y", folders: [], files: [] },
    ],
  };
  assert.deepEqual(flattenFolders(flat), [
    { name: "X", relPath: "X", depth: 0 },
    { name: "Y", relPath: "Y", depth: 0 },
  ]);
});

test("flattenFolders: pre-order con nesting → A, A/B, A/B/C, D", () => {
  // L'ordine è pre-order: prima il genitore poi i figli, poi il sibling.
  // I depth devono riflettere il livello di annidatura assoluto.
  assert.deepEqual(flattenFolders(moveTree), [
    { name: "A",   relPath: "A",     depth: 0 },
    { name: "B",   relPath: "A/B",   depth: 1 },
    { name: "C",   relPath: "A/B/C", depth: 2 },
    { name: "D",   relPath: "D",     depth: 0 },
  ]);
});

test("flattenFolders: un solo livello → depth 0", () => {
  // Verifica che un solo folder di primo livello abbia depth 0
  const single = {
    root_files: [],
    folders: [
      { name: "Solo", rel_path: "Solo", folders: [], files: [] },
    ],
  };
  assert.deepEqual(flattenFolders(single), [
    { name: "Solo", relPath: "Solo", depth: 0 },
  ]);
});

// ===========================================================================
// moveTargets — destinazioni valide per uno spostamento
//
// Regole (da brief):
//   1. Parti da flattenFolders(tree).
//   2. Escludi il parent corrente (spostare dove si è già = no-op/collisione).
//   3. Se isFolder: escludi anche sé stessa + ogni discendente.
//   4. Anteponi ROOT {name:"Documenti (radice)", relPath:"", depth:0}
//      TRANNE quando parentPath(itemRelPath) === "" (item è già al primo livello).
//
// Uso dell'albero moveTree sopra definito.
// ===========================================================================

// Costante radice per non ripetere l'oggetto nei test
const ROOT_OPTION = { name: "Documenti (radice)", relPath: "", depth: 0, disabled: false };

test("moveTargets: documento alla radice → root esclusa, offre tutte le cartelle", () => {
  // "doc.md" è un file root → parentPath = ""
  // Root esclusa (parent è già root). Nessuna cartella coincide col parent
  // (il parent "" non è una riga di flattenFolders) → nessuna disabled.
  // Nessuna esclusione self/discendenti (isFolder=false).
  // NOTA: name viene dal campo node.name (es. "C", non "A/B/C").
  const result = moveTargets(moveTree, "doc.md", false);
  assert.deepEqual(result, [
    { name: "A", relPath: "A",     depth: 0, disabled: false },
    { name: "B", relPath: "A/B",   depth: 1, disabled: false },
    { name: "C", relPath: "A/B/C", depth: 2, disabled: false },
    { name: "D", relPath: "D",     depth: 0, disabled: false },
  ]);
});

test("moveTargets: documento in A → A resta in lista come ancora disabled, root + A(disabled) + A/B + A/B/C + D", () => {
  // "A/note.md" è in A → parentPath = "A"
  // BUGFIX: A non viene più rimossa dalla lista (altrimenti B/C, che restano
  // visibili a depth 1/2, perderebbero l'ancora visiva del loro vero genitore
  // e sembrerebbero annidate sotto la riga precedente sbagliata — bug reale
  // osservato dal vivo). A resta, ma con disabled:true (non selezionabile).
  // Root inclusa (parent ≠ "").
  const result = moveTargets(moveTree, "A/note.md", false);
  assert.deepEqual(result, [
    ROOT_OPTION,
    { name: "A",   relPath: "A",     depth: 0, disabled: true },
    { name: "B",   relPath: "A/B",   depth: 1, disabled: false },
    { name: "C",   relPath: "A/B/C", depth: 2, disabled: false },
    { name: "D",   relPath: "D",     depth: 0, disabled: false },
  ]);
});

test("moveTargets: cartella A (primo livello) → A + discendenti esclusi (sorgente), root esclusa perché è il parent", () => {
  // "A" è al primo livello → parentPath = ""
  // Root non compare (parent è root, non una riga). A, A/B, A/B/C esclusi
  // del tutto: è la cartella sorgente stessa + i suoi discendenti, non un
  // genitore da ancorare.
  const result = moveTargets(moveTree, "A", true);
  assert.deepEqual(result, [
    { name: "D", relPath: "D", depth: 0, disabled: false },
  ]);
});

test("moveTargets: cartella A/B (annidato) → A/B + A/B/C esclusi (sorgente+discendente), A resta come ancora disabled, root inclusa", () => {
  // "A/B" → parentPath = "A"
  // A/B (self) e A/B/C (discendente) esclusi del tutto — regola 3.
  // A (parent) resta in lista come ancora disabled — stessa correzione di sopra.
  // Root inclusa (parent ≠ "").
  const result = moveTargets(moveTree, "A/B", true);
  assert.deepEqual(result, [
    ROOT_OPTION,
    { name: "A", relPath: "A", depth: 0, disabled: true },
    { name: "D", relPath: "D", depth: 0, disabled: false },
  ]);
});

test("moveTargets: cartella A/B/C (foglia profonda) → esclude self, A/B resta come ancora disabled, includi root, A, D", () => {
  // "A/B/C" → parentPath = "A/B"
  // A/B/C (self) esclusa del tutto. Nessun discendente.
  // A/B (parent) resta in lista come ancora disabled.
  // Root inclusa (parent = "A/B" ≠ "").
  const result = moveTargets(moveTree, "A/B/C", true);
  assert.deepEqual(result, [
    ROOT_OPTION,
    { name: "A", relPath: "A",   depth: 0, disabled: false },
    { name: "B", relPath: "A/B", depth: 1, disabled: true },
    { name: "D", relPath: "D",   depth: 0, disabled: false },
  ]);
});

test("moveTargets: cartella unica al primo livello → nessuna destinazione disponibile", () => {
  // Se l'unica cartella è D e vogliamo spostarla: parentPath = "", escludi D.
  // Root non preposta (parent = ""). Nessun'altra cartella → array vuoto.
  const singleFolderTree = {
    root_files: [],
    folders: [
      { name: "D", rel_path: "D", folders: [], files: [] },
    ],
  };
  const result = moveTargets(singleFolderTree, "D", true);
  assert.deepEqual(result, []);
});

test("moveTargets: guardia prefisso — 'AB' non viene esclusa quando si sposta 'A'", () => {
  // M-7: verifica che la logica usi `startsWith(itemRelPath + "/")` e NON
  // `startsWith(itemRelPath)`. Senza il "/" finale, spostare "A" escluderebbe
  // erroneamente anche "AB" (falso positivo del controllo discendente).
  //
  // Albero: "A" e "AB" sono fratelli (siblings) al primo livello.
  // Quando si sposta "A": "A" deve essere esclusa (è la sorgente);
  // "AB" deve essere INCLUSA (non è un discendente di "A").
  const prefixTree = {
    root_files: [],
    folders: [
      { name: "A",  rel_path: "A",  folders: [], files: [] },
      { name: "AB", rel_path: "AB", folders: [], files: [] },
    ],
  };
  const result = moveTargets(prefixTree, "A", true);
  // Solo "AB" deve essere presente nell'output; "A" (sorgente) è esclusa.
  assert.deepEqual(result, [{ name: "AB", relPath: "AB", depth: 0, disabled: false }]);
});
