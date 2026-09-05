// Spike 2 "throwaway" — cablaggio JS fra xterm.js e i comandi Rust/Tauri.
// Vanilla JS, nessun bundler: xterm.js e addon-fit sono già globali
// (window.Terminal, window.FitAddon) grazie ai <script> classici in
// index.html caricati prima di questo modulo.

// window.__TAURI__ è disponibile senza import perché tauri.conf.json ha
// "withGlobalTauri": true (stesso pattern del progetto v1).
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// --- Palette Campbell (la stessa di Windows Terminal per il profilo PowerShell) ---
// Definirla qui, non lasciarla al tema di default di xterm.js, è quello che
// fa "sembrare" questo terminale un vero pwsh in Windows Terminal invece di
// un terminale xterm.js "generico" (nero/bianco).
const campbellTheme = {
  background: "#0C0C0C",
  foreground: "#CCCCCC",
  cursor: "#FFFFFF",
  black: "#0C0C0C",
  red: "#C50F1F",
  green: "#13A10E",
  yellow: "#C19C00",
  blue: "#0037DA",
  magenta: "#881798",
  cyan: "#3A96DD",
  white: "#CCCCCC",
  brightBlack: "#767676",
  brightRed: "#E74856",
  brightGreen: "#16C60C",
  brightYellow: "#F9F1A5",
  brightBlue: "#3B78FF",
  brightMagenta: "#B4009E",
  brightCyan: "#61D6D6",
  brightWhite: "#F2F2F2",
};

const term = new Terminal({
  cursorBlink: true,
  scrollback: 5000,
  theme: campbellTheme,
  fontFamily: "Cascadia Mono, Consolas, monospace",
  fontSize: 14,
  allowProposedApi: true, // richiesto da alcune API dei parser OSC custom usate sotto
});

const fitAddon = new FitAddon.FitAddon();
term.loadAddon(fitAddon);
term.open(document.getElementById("terminal"));
fitAddon.fit();
term.focus();

// --- Decodifica dei chunk pty-out (base64 -> byte grezzi) -----------------
// Il lato Rust invia base64 invece di una stringa JS "normale" perché un
// chunk da 8 KiB letto dalla pty può tagliare a metà un carattere UTF-8
// multi-byte: convertire quei byte "a metà" in una stringa JS li
// corromperebbe. term.write() invece accetta direttamente un Uint8Array e fa
// la propria decodifica UTF-8 incrementale corretta anche attraverso più
// chiamate consecutive.
function base64ToUint8Array(b64) {
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

// --- Indicatore "ultimo comando intercettato" nella barra superiore -------
const lastCommandEl = document.getElementById("last-command");
const dotEl = document.getElementById("intercept-dot");
let dotTimer = null;

function showIntercepted(rawLine) {
  lastCommandEl.textContent = "ultimo: " + rawLine;
  dotEl.classList.add("dot-active");
  clearTimeout(dotTimer);
  dotTimer = setTimeout(() => dotEl.classList.remove("dot-active"), 2000);
}

// Canale primario: OSC custom 9001, scritto dallo shell host C# come
// "ESC ] 9001 ; lare ; intercept ; <riga grezza> ESC \" ad ogni riga "/"
// intercettata (vedi Repl.cs nello spike 1). Il payload che arriva qui è
// tutto ciò che sta fra "9001;" e il terminatore, cioè "lare;intercept;<riga>".
term.parser.registerOscHandler(9001, (data) => {
  const parts = data.split(";");
  if (parts[0] === "lare" && parts[1] === "intercept") {
    const rawLine = parts.slice(2).join(";");
    showIntercepted(rawLine);
  }
  return true; // sequenza gestita: xterm.js non deve fare altro con questa OSC
});

// Canale di riserva: cambio titolo console. Spiegazione del perché esiste
// (vedi anche il commento in Repl.cs/Program.cs lato C#): ConPTY (la pseudo-
// console di Windows su cui gira la shell) inoltra al lato lettura solo le
// sequenze VT che riconosce, e potrebbe scartare una OSC "custom" come la
// 9001 perché non la conosce — un limite documentato di ConPTY, non un bug
// di questo spike. Il cambio titolo (OSC 0/2, "ESC ] 0 ; testo BEL") è invece
// una sequenza standard che ConPTY DEVE ri-emettere (è così che Windows
// Terminal aggiorna il titolo della scheda), quindi funziona anche se la 9001
// venisse inghiottita. Se `title` inizia con "lare;intercept;" trattiamolo
// come lo stesso segnale della OSC 9001.
term.onTitleChange((title) => {
  const marker = "lare;intercept;";
  if (title.startsWith(marker)) {
    showIntercepted(title.slice(marker.length));
  }
});

// --- Bottom bar: bottoni /help /library /aichat /config --------------------
// Cliccare un bottone scrive il comando nella shell "come se l'utente lo
// avesse digitato" (testo + invio) e riporta il focus sul terminale, così si
// può continuare a digitare subito dopo senza dover ricliccare dentro l'area
// del terminale.
document.querySelectorAll(".slash-btn").forEach((btn) => {
  btn.addEventListener("click", () => {
    const cmd = btn.dataset.cmd;
    invoke("pty_write", { data: cmd + "\r" }).catch((err) => {
      console.error("pty_write fallita:", err);
    });
    term.focus();
  });
});

// --- Orologio nella barra superiore ----------------------------------------
const clockEl = document.getElementById("clock");
function tickClock() {
  const now = new Date();
  const pad = (n) => String(n).padStart(2, "0");
  clockEl.textContent = `${pad(now.getHours())}:${pad(now.getMinutes())}:${pad(now.getSeconds())}`;
}
tickClock();
setInterval(tickClock, 1000);

// --- Avvio: registra i listener PRIMA di avviare la pty --------------------
// Ordine importante: se invocassimo pty_spawn prima di aver registrato
// listen('pty-out', ...), i primissimi byte scritti dalla shell (il banner
// "Lare Terminal 2.0 - spike host PowerShell", ecc.) rischierebbero di essere
// emessi da Rust prima che il frontend sia in ascolto, e andrebbero persi.
async function main() {
  const shellPathEl = document.getElementById("shell-path");

  await listen("pty-out", (event) => {
    term.write(base64ToUint8Array(event.payload));
  });

  await listen("pty-exit", (event) => {
    term.write("\r\n\x1b[31m[lare] shell terminata (exit code: " + event.payload + ")\x1b[0m\r\n");
    shellPathEl.textContent = "shell terminata";
  });

  try {
    const info = await invoke("shell_info");
    shellPathEl.textContent = "shell: " + info.path + (info.isSpikeHost ? " (spike host)" : " (fallback)");
  } catch (err) {
    shellPathEl.textContent = "shell: ? (" + err + ")";
  }

  try {
    await invoke("pty_spawn", { cols: term.cols, rows: term.rows, shell: null });
  } catch (err) {
    term.write("\r\n\x1b[31m[lare] pty_spawn fallita: " + err + "\x1b[0m\r\n");
    return; // niente resize/onData da collegare se la pty non è mai partita
  }

  term.onData((data) => {
    invoke("pty_write", { data }).catch((err) => console.error("pty_write fallita:", err));
  });

  // ResizeObserver + debounce 50ms: FitAddon ricalcola cols/rows quando la
  // finestra (quindi #terminal) cambia dimensione, e comunichiamo la nuova
  // dimensione alla pty lato Rust con pty_resize. Il debounce evita di
  // spammare invoke() ad ogni singolo pixel durante un drag di resize.
  let resizeTimer = null;
  const resizeObserver = new ResizeObserver(() => {
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(() => {
      fitAddon.fit();
      invoke("pty_resize", { cols: term.cols, rows: term.rows }).catch((err) => {
        console.error("pty_resize fallita:", err);
      });
    }, 50);
  });
  resizeObserver.observe(document.getElementById("terminal"));
}

main();
