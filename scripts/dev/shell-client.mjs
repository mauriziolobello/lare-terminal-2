#!/usr/bin/env node
// shell-client.mjs — Client di SVILUPPO del canale shell (piano 2a).
//
// Fa ciò che farà lare-shell (piano 2b), in piccolo: Hello{role:"shell"},
// UN Command, e poi risponde ai messaggi dell'orchestratore come una host:
//   tool_confirm_request → chiede [Y/n] su stdin
//   exec_in_shell        → esegue con pwsh (capture:true cattura, false no) e manda exec_result
//   chunk/done/error     → stampa; done/error chiudono
// Uso:
//   node scripts/dev/shell-client.mjs [--config-dir "Test Run/Configuration"] [--session s1] -- '/ai "elenca i file"'
// Legge token e ws_port dalla cartella di configurazione (mai variabili d'ambiente, D6).

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { createInterface } from "node:readline";

const argv = process.argv.slice(2);
function flag(name, def) { const i = argv.indexOf(name); return i >= 0 ? argv[i + 1] : def; }
const configDir = flag("--config-dir", join("Test Run", "Configuration"));
const sessionId = flag("--session", `dev-${process.pid}`);
const sep = argv.indexOf("--");
const input = (sep >= 0 ? argv.slice(sep + 1) : argv.filter((a, i) => !a.startsWith("--") && argv[i - 1] !== "--config-dir" && argv[i - 1] !== "--session")).join(" ");
if (!input) { console.error("manca il comando, es.: -- '/ping'"); process.exit(2); }

const token = readFileSync(join(configDir, "token"), "utf8").trim();
const port = JSON.parse(readFileSync(join(configDir, "startup.json"), "utf8")).ws_port ?? 7331;
const rl = createInterface({ input: process.stdin, output: process.stdout });
const ask = (q) => new Promise((res) => rl.question(q, res));

const ws = new WebSocket(`ws://127.0.0.1:${port}`);
const sendJson = (o) => ws.send(JSON.stringify(o));
const CWD_MARK = "__LARE_CWD__";

function execInShell(command, capture) {
  // La cwd è quella del processo: ogni comando parte da lì (una host vera tiene il runspace).
  const script = `${command}\nWrite-Output ('${CWD_MARK}' + (Get-Location).Path)`;
  const r = spawnSync("pwsh", ["-NoProfile", "-NonInteractive", "-Command", script], {
    cwd: process.cwd(), encoding: "utf8", stdio: capture ? ["ignore", "pipe", "pipe"] : ["inherit", "inherit", "inherit"],
  });
  let output = "", cwd = process.cwd();
  if (capture) {
    const lines = ((r.stdout ?? "") + (r.stderr ?? "")).split(/\r?\n/);
    const mark = lines.findLast((l) => l.startsWith(CWD_MARK));
    if (mark) cwd = mark.slice(CWD_MARK.length);
    output = lines.filter((l) => !l.startsWith(CWD_MARK)).join("\n");
    process.stdout.write(output.endsWith("\n") ? output : output + "\n");
  }
  return { exit_code: r.status ?? -1, output, cwd };
}

ws.addEventListener("open", () => {
  sendJson({ type: "hello", token, role: "shell", session_id: sessionId, cwd: process.cwd(), version: "dev-client" });
});
ws.addEventListener("message", async (ev) => {
  const msg = JSON.parse(ev.data);
  switch (msg.type) {
    case "server_info":
      sendJson({ type: "command", id: `cmd-${Date.now()}`, input, input_mode: "keyboard", command_type: "auto", cwd: process.cwd(), web_search: false });
      break;
    case "tool_confirm_request": {
      const a = (await ask(`\n${msg.commands}\nEseguire? [Y/n] `)).trim().toLowerCase();
      sendJson({ type: "tool_confirm_response", id: msg.id, accept: a === "" || a === "y" || a === "s" });
      break;
    }
    case "exec_in_shell": {
      const r = execInShell(msg.command, msg.capture);
      sendJson({ type: "exec_result", turn_id: msg.turn_id, exec_id: msg.exec_id, ...r });
      break;
    }
    case "chunk": console.log(msg.content); break;
    case "done": console.log(`[done exit_code=${msg.exit_code}]`); ws.close(); rl.close(); break;
    case "error": console.error(`[error ${msg.code}] ${msg.message}`); ws.close(); rl.close(); process.exitCode = 1; break;
    default: console.log(`[${msg.type}]`, JSON.stringify(msg));
  }
});
ws.addEventListener("error", (e) => { console.error("ws error:", e.message ?? e); process.exit(1); });
ws.addEventListener("close", () => process.exit());
