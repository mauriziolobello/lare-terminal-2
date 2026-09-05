// Spike 2 "throwaway" per Lare Terminal 2.0: una finestra Tauri che ospita un
// vero terminale (xterm.js nel webview) collegato via ConPTY a un processo
// figlio (di norma lare-shell-spike.exe, lo spike 1). Questo file è
// volutamente commentato in modo didattico: l'obiettivo non è solo "far
// funzionare la pty", ma spiegare PERCHÉ ogni pezzo è fatto così.
//
// NON abilitiamo `windows_subsystem = "windows"` (che in v1 nasconde la
// console e stacca stdout/stderr in release): qui stderr è la prova che lo
// smoke-test dello spike richiede di vedere ("mostra la riga della shell
// usata"), quindi la console resta sempre attaccata.

use std::io::{Read, Write};
use std::sync::Mutex;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use tauri::{AppHandle, Emitter, State};

/// Percorso assoluto, calcolato una sola volta all'avvio, della shell da
/// lanciare di default e flag "è lo spike 1 lare-shell-spike.exe?" (serve per
/// decidere se aggiungere l'argomento `--no-bars`: pwsh.exe non conosce quel
/// flag e lo interpreterebbe come nome di uno script da eseguire).
/// È uno stato "di sola lettura" dopo l'avvio: non serve un Mutex, Tauri
/// consente di gestire (app.manage) qualunque tipo Send+Sync+'static.
struct ShellInfo {
    path: String,
    is_spike_host: bool,
}

/// Una sessione pty attiva. Nota rispetto alla bozza iniziale dello spike:
/// QUI NON teniamo un campo `child` dentro questa struct (deviazione
/// documentata anche nel README, sezione "Semplificazioni note"). Il motivo:
/// ConPTY su Windows NON segnala la fine del processo figlio chiudendo il lato
/// di lettura della pty (a differenza di una pipe Unix "vera") — il reader
/// resterebbe bloccato in `read()` finché il pseudo-console stesso non viene
/// chiuso. L'unico modo affidabile per sapere "il processo figlio è
/// terminato" è chiamare `Child::wait()`, che richiede possesso esclusivo
/// (&mut) del child: lo spostiamo quindi per intero dentro il thread dedicato
/// `spawn_exit_watcher`, che lo "consuma" chiamando wait() e poi segnala
/// l'uscita al frontend. Master e writer restano invece qui, protetti dal
/// Mutex nello stato Tauri, perché servono per pty_write/pty_resize finché la
/// sessione è viva.
struct PtySession {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
}

/// Stato Tauri condiviso: `Option` perché prima di pty_spawn non c'è ancora
/// nessuna sessione; `Mutex` perché più comandi invoke (pty_write, pty_resize)
/// possono arrivare da thread diversi del runtime Tauri.
struct PtyState(Mutex<Option<PtySession>>);

/// Calcola il percorso di default della shell e se si tratta dello spike 1.
///
/// Precedenza (dal più al meno specifico):
///   1. argomento `--shell <path>` passato sulla riga di comando di QUESTO
///      processo (utile per testare con una shell diversa senza ricompilare);
///   2. il percorso assoluto noto dello spike 1 (lare-shell-spike.exe), se il
///      file esiste;
///   3. fallback a `pwsh.exe` (deve essere nel PATH di sistema).
fn resolve_default_shell() -> ShellInfo {
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--shell") {
        if let Some(custom) = args.get(pos + 1) {
            return ShellInfo {
                is_spike_host: is_lare_shell_host(custom),
                path: custom.clone(),
            };
        }
    }

    // Percorso fisso: calcolarlo "relativo a src-tauri" in fase di build
    // sarebbe fragile (dipende da dove finisce l'artefatto compilato), quindi
    // usiamo direttamente il percorso assoluto noto della cartella spikes/
    // di questo stesso repository, come richiesto dalla specifica.
    const SPIKE_HOST_PATH: &str = r"C:\Users\Maurizio\Documents\Progetti\Lare Terminal 2.0\spikes\lare-shell-host\bin\Release\net10.0\lare-shell-spike.exe";
    if std::path::Path::new(SPIKE_HOST_PATH).exists() {
        return ShellInfo {
            path: SPIKE_HOST_PATH.to_string(),
            is_spike_host: true,
        };
    }

    ShellInfo {
        path: "pwsh.exe".to_string(),
        is_spike_host: false,
    }
}

/// true se `path` punta (per nome file, case-insensitive) a
/// lare-shell-spike.exe. Usata per decidere se aggiungere `--no-bars`
/// all'avvio: quel flag ha senso SOLO per il nostro host C#, mai per pwsh.exe.
fn is_lare_shell_host(path: &str) -> bool {
    std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.eq_ignore_ascii_case("lare-shell-spike.exe"))
        .unwrap_or(false)
}

/// Comando invoke: espone al frontend la shell risolta all'avvio, così la
/// barra superiore può mostrarla (uno dei quattro obiettivi dello spike:
/// "la chrome è HTML puro", qui applicato mostrando un dato preso da Rust).
#[tauri::command]
fn shell_info(shell: State<'_, ShellInfo>) -> serde_json::Value {
    serde_json::json!({
        "path": shell.path,
        "isSpikeHost": shell.is_spike_host,
    })
}

/// Avvia la pseudo-console e ci lancia dentro la shell (di default quella
/// risolta da resolve_default_shell, altrimenti quella passata da JS).
/// `cols`/`rows` arrivano da xterm.js (term.cols/term.rows) così la pty nasce
/// già della dimensione giusta invece che con un default arbitrario.
#[tauri::command]
fn pty_spawn(
    app: AppHandle,
    state: State<'_, PtyState>,
    shell_info: State<'_, ShellInfo>,
    cols: u16,
    rows: u16,
    shell: Option<String>,
) -> Result<(), String> {
    let mut guard = state.0.lock().map_err(|e| e.to_string())?;
    if guard.is_some() {
        return Err("una sessione pty è già attiva".to_string());
    }

    // ConPTY rifiuta dimensioni 0: clampiamo a un minimo ragionevole invece di
    // propagare un errore criptico se il frontend, per qualche motivo, invoca
    // questo comando prima che #terminal abbia una dimensione reale.
    let cols = cols.max(2);
    let rows = rows.max(1);

    let (shell_path, add_no_bars) = match shell {
        Some(custom) => {
            let is_spike = is_lare_shell_host(&custom);
            (custom, is_spike)
        }
        None => (shell_info.path.clone(), shell_info.is_spike_host),
    };

    eprintln!("[lare-terminal-window] avvio pty con shell: {shell_path} (--no-bars: {add_no_bars})");

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("openpty fallita: {e}"))?;

    let mut cmd = CommandBuilder::new(&shell_path);
    if add_no_bars {
        cmd.arg("--no-bars");
    }
    // Cartella di lavoro iniziale: la home dell'utente, come farebbe un
    // terminale "vero" aperto da Explorer/Start Menu.
    if let Ok(home) = std::env::var("USERPROFILE") {
        cmd.cwd(home);
    }

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("spawn_command fallita ({shell_path}): {e}"))?;

    // La nostra copia del lato "slave" della pty non serve più una volta che
    // il processo figlio l'ha ereditata: la chiudiamo esplicitamente. Se la
    // tenessimo in vita, alcuni backend di portable-pty non emettono mai un
    // EOF di lettura anche dopo la chiusura "logica" del lato slave del figlio.
    drop(pair.slave);

    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("take_writer fallita: {e}"))?;
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("try_clone_reader fallita: {e}"))?;

    spawn_reader_thread(app.clone(), reader);
    spawn_exit_watcher(app, child);

    *guard = Some(PtySession {
        master: pair.master,
        writer,
    });

    Ok(())
}

/// Thread che legge in continuazione dalla pty e inoltra i byte al frontend
/// via evento `pty-out`. Vive finché la lettura non fallisce (pty chiusa) —
/// vedi il commento su PtySession sul perché NON usiamo un eventuale EOF qui
/// come segnale "il processo è terminato" (non arriva mai, su Windows/ConPTY,
/// finché la pty stessa non viene chiusa).
fn spawn_reader_thread(app: AppHandle, mut reader: Box<dyn Read + Send>) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break, // pty chiusa dal lato lettura: usciamo in silenzio.
                Ok(n) => {
                    // Base64 invece di provare a convertire i byte grezzi in una
                    // stringa UTF-8 qui: un chunk da 8 KiB può tagliare a metà un
                    // carattere multi-byte (es. una lettera accentata o un
                    // carattere Unicode >1 byte emesso da PowerShell), e String::
                    // from_utf8 fallirebbe o corromperebbe il carattere. xterm.js,
                    // dal lato JS, sa gestire correttamente un flusso di byte
                    // spezzato in punti arbitrari (term.write accetta Uint8Array).
                    let encoded = BASE64.encode(&buf[..n]);
                    if app.emit("pty-out", encoded).is_err() {
                        break; // finestra chiusa: non ha senso continuare a leggere.
                    }
                }
                Err(_) => break, // pty chiusa/errore: il watcher di uscita informerà il frontend.
            }
        }
    });
}

/// Thread che possiede il processo figlio e blocca su `wait()` finché non
/// termina, poi emette `pty-exit` con l'exit code al frontend. È QUESTO il
/// meccanismo affidabile di rilevazione fine-processo su Windows/ConPTY (vedi
/// il commento su PtySession).
fn spawn_exit_watcher(app: AppHandle, mut child: Box<dyn Child + Send + Sync>) {
    std::thread::spawn(move || {
        let code = match child.wait() {
            Ok(status) => status.exit_code() as i64,
            Err(_) => -1,
        };
        eprintln!("[lare-terminal-window] processo figlio terminato, exit code: {code}");
        let _ = app.emit("pty-exit", code);
    });
}

/// Scrive nella pty i byte digitati/incollati nel terminale (term.onData lato
/// JS). `flush()` esplicito: senza, l'output potrebbe restare bufferizzato
/// abbastanza a lungo da far sembrare la digitazione "in ritardo".
#[tauri::command]
fn pty_write(state: State<'_, PtyState>, data: String) -> Result<(), String> {
    let mut guard = state.0.lock().map_err(|e| e.to_string())?;
    let session = guard.as_mut().ok_or("nessuna sessione pty attiva")?;
    session
        .writer
        .write_all(data.as_bytes())
        .map_err(|e| e.to_string())?;
    session.writer.flush().map_err(|e| e.to_string())
}

/// Ridimensiona la pseudo-console quando #terminal cambia dimensione (vedi il
/// ResizeObserver + FitAddon lato JS). `resize` su MasterPty prende `&self`
/// (non serve `&mut`): l'implementazione sottostante parla direttamente con
/// l'handle ConPTY di Windows, non con uno stato Rust che richiederebbe
/// esclusività.
#[tauri::command]
fn pty_resize(state: State<'_, PtyState>, cols: u16, rows: u16) -> Result<(), String> {
    let guard = state.0.lock().map_err(|e| e.to_string())?;
    let session = guard.as_ref().ok_or("nessuna sessione pty attiva")?;
    session
        .master
        .resize(PtySize {
            rows: rows.max(1),
            cols: cols.max(2),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())
}

fn main() {
    // NOTA sul nome: non chiamiamo questa variabile `shell_info` perché
    // esiste anche una funzione (il comando invoke) con lo stesso nome poco
    // sotto — una variabile locale con lo stesso nome di una funzione
    // "nasconde" quella funzione nel resto dello scope (incluso dentro la
    // macro tauri::generate_handler! più sotto, che altrimenti smetterebbe
    // di vedere il comando come funzione, un errore effettivamente incontrato
    // durante lo sviluppo di questo spike).
    let resolved_shell = resolve_default_shell();
    // Stampata SEMPRE, prima ancora che il runtime Tauri parta: è la riga che
    // lo smoke-test (step 5 della specifica) verifica su stderr. Se in futuro
    // il webview/JS avesse un problema prima di invocare pty_spawn, questa
    // riga resta comunque la prova che il lato Rust ha risolto correttamente
    // la shell da usare.
    eprintln!(
        "[lare-terminal-window] shell risolta all'avvio: {} (spike host: {})",
        resolved_shell.path, resolved_shell.is_spike_host
    );

    tauri::Builder::default()
        .manage(resolved_shell)
        .manage(PtyState(Mutex::new(None)))
        .invoke_handler(tauri::generate_handler![
            shell_info,
            pty_spawn,
            pty_write,
            pty_resize,
        ])
        .run(tauri::generate_context!())
        .expect("[lare-terminal-window] errore runtime Tauri");
}
