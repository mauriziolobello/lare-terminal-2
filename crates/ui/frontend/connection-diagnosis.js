// connection-diagnosis.js — logica pura per il report di auto-diagnosi connessione.
//
// SRP: dato il risultato dei check (token impostato, porta aperta), costruisce
// un report Markdown leggibile dall'utente. Nessun DOM, nessun I/O, nessuna
// dipendenza Tauri → testabile con node:test.

const ORCH_CMD =
  "```powershell\n$env:LARE_TOKEN = \"lare-dev\"\ncargo run -p orchestrator\n```";

const TOKEN_CMD =
  "```powershell\n$env:LARE_TOKEN = \"lare-dev\"\n```";

/**
 * Costruisce il report Markdown di auto-diagnosi della connessione all'orchestrator.
 *
 * @param {boolean} tokenSet - true se LARE_TOKEN è impostata nel processo UI.
 * @param {boolean} portOpen - true se la porta TCP 7331 ha accettato la connessione.
 * @returns {string} Stringa Markdown pronta per essere mostrata in una finestra.
 */
export function buildDiagnosisMarkdown(tokenSet, portOpen) {
  const tokenIcon = tokenSet ? "✅" : "❌";
  const portIcon  = portOpen  ? "✅" : "❌";

  const lines = [
    "## Diagnosi connessione Lare",
    "",
    "| Check | Risultato |",
    "|---|---|",
    `| \`LARE_TOKEN\` impostata | ${tokenIcon} ${tokenSet ? "Sì" : "No"} |`,
    `| Orchestrator in ascolto su \`:7331\` | ${portIcon} ${portOpen ? "Sì" : "No"} |`,
    "",
    "### Come risolvere",
    "",
  ];

  if (!tokenSet) {
    lines.push(
      "**`LARE_TOKEN` non impostata.** Avvia l'UI con la variabile impostata:",
      "",
      TOKEN_CMD,
      "",
    );
  }

  if (!portOpen) {
    lines.push(
      "**Orchestrator non in ascolto.** Avvialo in un terminale separato:",
      "",
      ORCH_CMD,
      "",
    );
  }

  if (tokenSet && portOpen) {
    lines.push(
      "**Entrambi i check sono OK** ma la connessione non si stabilisce.",
      "Il token usato dall'orchestrator potrebbe non corrispondere.",
      "Riavvia entrambi i processi con la stessa `LARE_TOKEN`:",
      "",
      ORCH_CMD,
      "",
    );
  }

  lines.push("---", "*La UI continua a riprovare la connessione automaticamente.*");

  return lines.join("\n");
}
