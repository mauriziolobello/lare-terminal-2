//! Wrapper puro attorno a `portable-pty` (ConPTY su Windows, spec §5): apre
//! una pseudo-console, ci lancia un processo, e inoltra output/uscita a un
//! `PtyOutputSink` iniettato — mai `AppHandle` direttamente qui, così questo
//! modulo si testa con `cargo test -p ui` senza un runtime Tauri (spec §10:
//! "pty plumbing con una shell finta").
//!
//! Confine OOP: `PtyOutputSink` è l'interfaccia (trait) fra "cosa è successo
//! nella pty" e "come lo sa il resto del mondo" — nel prodotto la implementa
//! un `AppHandle` Tauri (emette gli eventi `pty-out`/`pty-exit`, vedi
//! `main.rs`), nei test un fake che registra le chiamate in un `Vec`. Stesso
//! schema di `IProcessStarter` nella host C# (composizione, non eredità).
//!
//! Basato sul codice verificato dal vivo dello spike 2
//! (`spikes/lare-terminal-window/src-tauri/src/main.rs`), adattato per
//! separare la pura logica pty (qui) dai comandi Tauri (`main.rs`).

use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

/// Confine testabile: nel prodotto lo implementa `AppHandle` (emette eventi
/// Tauri), nei test un fake che registra le chiamate.
///
/// # Requisito VT: chi implementa questo trait deve rispondere alla DSR
/// Conhost (il backend Windows di ConPTY) manda `ESC[6n` (Device Status
/// Report — richiesta posizione cursore) al primo avvio del processo figlio,
/// e RESTA BLOCCATO finché non riceve una risposta CPR (`ESC[<row>;<col>R`)
/// — nessun altro output arriva, e il figlio non termina, fino a quel
/// momento (verificato dal vivo su questa macchina: `cmd /c echo hello`
/// resta appeso a tempo indefinito senza risposta). Nel prodotto risponde
/// xterm.js (supporto DSR nativo, via `pty_write`); nei test un fake deve
/// simulare la stessa risposta — vedi `answer_cursor_position_query` sotto.
pub trait PtyOutputSink: Send + Sync + 'static {
    /// Chunk di output pty, già codificato base64 — vedi `spawn_reader_thread`
    /// sul perché base64 e non una `String` UTF-8 diretta.
    fn on_output(&self, base64_chunk: &str);
    /// Il processo figlio è terminato, con questo exit code (-1 se non determinabile).
    ///
    /// Nota d'ordine: questo può arrivare PRIMA che l'ultimo chunk di
    /// `on_output` sia stato consegnato (`spawn_exit_watcher` e
    /// `spawn_reader_thread` sono due thread indipendenti) — un consumatore
    /// che tratta `on_exit` come "tutto l'output è già arrivato" può perdere
    /// le ultime righe. Task 4: il frontend non deve assumere quest'ordine.
    fn on_exit(&self, code: i64);
}

/// Sessione pty attiva: master (per il resize) + writer (per scrivere
/// input). NIENTE campo `child` qui — vedi `spawn_exit_watcher`:
/// `Child::wait()` richiede possesso esclusivo e vive nel suo thread
/// dedicato, non nello stato condiviso.
pub struct PtySession {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
}

/// Stato condiviso: `None` finché nessuna sessione è mai partita; tornato a
/// `None` dall'exit watcher quando il processo termina, così una nuova
/// `spawn` (bottone "riavvia") trova lo stato libero invece di rifiutarsi
/// con "sessione già attiva" (bug mai corretto nello spike 2, throwaway).
pub type SharedPtyState = Arc<Mutex<Option<PtySession>>>;

pub fn shared_state() -> SharedPtyState {
    Arc::new(Mutex::new(None))
}

/// Apre la pseudo-console e ci lancia `exe` con `args`, in `cwd` (default: la
/// cwd del processo). Errore se una sessione è già attiva ("una finestra,
/// una sessione", spec §5) o se lo spawn fallisce.
pub fn spawn<S: PtyOutputSink>(
    state: &SharedPtyState,
    sink: Arc<S>,
    exe: &str,
    args: &[String],
    cwd: Option<&Path>,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    if guard.is_some() {
        return Err("una sessione pty è già attiva".to_string());
    }

    // ConPTY rifiuta dimensioni 0: clampiamo invece di propagare un errore
    // criptico se il frontend invoca questo comando prima che #terminal
    // abbia una dimensione reale.
    let cols = cols.max(2);
    let rows = rows.max(1);

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("openpty fallita: {e}"))?;

    let mut cmd = CommandBuilder::new(exe);
    for a in args {
        cmd.arg(a);
    }
    if let Some(dir) = cwd {
        cmd.cwd(dir);
    }

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("spawn_command fallita ({exe}): {e}"))?;

    // Il lato slave non serve più al padre una volta ereditato dal figlio;
    // tenerlo in vita impedisce a certi backend di segnalare mai un EOF di
    // lettura (spike 2).
    drop(pair.slave);

    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("take_writer fallita: {e}"))?;
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("try_clone_reader fallita: {e}"))?;

    spawn_reader_thread(sink.clone(), reader);
    spawn_exit_watcher(sink, child, state.clone());

    *guard = Some(PtySession {
        master: pair.master,
        writer,
    });
    Ok(())
}

/// Scrive `data` nella pty attiva (errore se nessuna sessione è viva).
pub fn write(state: &SharedPtyState, data: &[u8]) -> Result<(), String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    let session = guard.as_mut().ok_or("nessuna sessione pty attiva")?;
    session.writer.write_all(data).map_err(|e| e.to_string())?;
    session.writer.flush().map_err(|e| e.to_string())
}

/// Ridimensiona la pty attiva.
pub fn resize(state: &SharedPtyState, cols: u16, rows: u16) -> Result<(), String> {
    let guard = state.lock().map_err(|e| e.to_string())?;
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

fn spawn_reader_thread<S: PtyOutputSink>(sink: Arc<S>, mut reader: Box<dyn Read + Send>) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break, // pty chiusa dal lato lettura.
                Ok(n) => sink.on_output(&BASE64.encode(&buf[..n])),
                Err(_) => break,
            }
        }
    });
}

/// Possiede il processo figlio e blocca su `wait()` finché non termina:
/// l'UNICO segnale affidabile di fine-processo su Windows/ConPTY (che non
/// segnala mai un EOF di lettura alla chiusura del figlio — spike 2, fatto
/// tecnico #1). Libera lo stato PRIMA di notificare: un `on_exit` che
/// richiama subito `spawn` (bottone "riavvia") deve trovare la sessione già
/// sgombra.
fn spawn_exit_watcher<S: PtyOutputSink>(
    sink: Arc<S>,
    mut child: Box<dyn Child + Send + Sync>,
    state: SharedPtyState,
) {
    std::thread::spawn(move || {
        let code = match child.wait() {
            Ok(status) => status.exit_code() as i64,
            Err(_) => -1,
        };
        if let Ok(mut guard) = state.lock() {
            *guard = None;
        }
        sink.on_exit(code);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
    use std::sync::Mutex as StdMutex;
    use std::time::Duration;

    #[derive(Default)]
    struct FakeSink {
        outputs: StdMutex<Vec<String>>,
        exit_code: AtomicI64,
        exited: AtomicBool,
    }
    impl PtyOutputSink for FakeSink {
        fn on_output(&self, chunk: &str) {
            self.outputs.lock().unwrap().push(chunk.to_string());
        }
        fn on_exit(&self, code: i64) {
            self.exit_code.store(code, Ordering::SeqCst);
            self.exited.store(true, Ordering::SeqCst);
        }
    }

    fn wait_until<F: Fn() -> bool>(cond: F) {
        for _ in 0..100 {
            if cond() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("timeout in attesa della condizione");
    }

    /// Decodifica un chunk base64 (come arriva a `PtyOutputSink::on_output`)
    /// nella stringa grezza — helper condiviso dai test sotto.
    fn decode(b64: &str) -> String {
        String::from_utf8_lossy(
            &base64::engine::general_purpose::STANDARD
                .decode(b64)
                .unwrap(),
        )
        .into_owned()
    }

    /// Il `FakeSink` sta al posto di xterm.js: conhost (backend Windows di
    /// ConPTY) manda `ESC[6n` (DSR, richiesta posizione cursore) all'avvio
    /// del figlio e blocca l'intera pty — niente altro output, il processo
    /// non termina — finché non arriva una risposta CPR (`ESC[<r>;<c>R`).
    /// xterm.js risponde nativamente nel prodotto; qui simuliamo la stessa
    /// risposta scrivendo una CPR fittizia appena vediamo la query nei byte
    /// grezzi (`ESC [ 6 n` = `[27, 91, 54, 110]`) — verificato dal vivo:
    /// senza questa risposta `cmd /c echo hello` resta appeso a tempo
    /// indefinito su questa macchina (nessun timeout lo risolve).
    fn answer_cursor_position_query(state: &SharedPtyState, sink: &FakeSink) {
        for _ in 0..40 {
            let saw_dsr = sink
                .outputs
                .lock()
                .unwrap()
                .iter()
                .any(|b64| decode(b64).contains("\x1b[6n"));
            if saw_dsr {
                let _ = write(state, b"\x1b[24;1R");
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        // Nessuna DSR vista: non blocchiamo il test, la wait_until successiva
        // farà comunque scattare un panic diagnostico se il figlio è appeso.
    }

    #[test]
    fn spawn_scrive_e_riceve_output_di_una_shell_finta() {
        let state = shared_state();
        let sink = std::sync::Arc::new(FakeSink::default());
        // "cmd /c echo hello": shell finta disponibile su ogni Windows —
        // niente dipendenza da lare-shell.exe (spec §10).
        spawn(
            &state,
            sink.clone(),
            "cmd",
            &["/c".into(), "echo hello".into()],
            None,
            80,
            24,
        )
        .unwrap();

        answer_cursor_position_query(&state, &sink);
        wait_until(|| sink.exited.load(Ordering::SeqCst));
        assert_eq!(sink.exit_code.load(Ordering::SeqCst), 0);

        // `on_exit` può arrivare prima che l'ultimo chunk di output sia stato
        // consegnato (due thread indipendenti, vedi doc di `PtyOutputSink`) —
        // eseguiamo il polling sul contenuto invece di decodificare una volta
        // sola subito dopo `wait_until(exited)`.
        wait_until(|| {
            sink.outputs
                .lock()
                .unwrap()
                .iter()
                .map(|b64| decode(b64))
                .collect::<String>()
                .contains("hello")
        });

        // Lo stato è tornato libero: una nuova spawn deve poter ripartire
        // (bottone "riavvia" di terminal.js, Task 4) — non il bug "sessione
        // già attiva" mai corretto nello spike 2 (throwaway).
        assert!(state.lock().unwrap().is_none());
    }

    #[test]
    fn spawn_rifiuta_una_seconda_sessione_mentre_la_prima_e_attiva() {
        let state = shared_state();
        let sink = std::sync::Arc::new(FakeSink::default());
        spawn(&state, sink.clone(), "cmd", &["/c".into(), "pause".into()], None, 80, 24).unwrap();
        let err = spawn(&state, sink.clone(), "cmd", &["/c".into(), "echo x".into()], None, 80, 24)
            .unwrap_err();
        assert!(err.contains("già attiva"));

        // pulizia: risponde alla DSR di "pause" (altrimenti resta appesa,
        // stesso motivo della prima sessione), aspetta che il prompt di
        // "pause" sia davvero arrivato (altrimenti l'invio può corrergli
        // davanti — race osservata dal vivo), poi un invio la termina — non
        // lasciamo un cmd.exe orfano. `wait_until` finale dimostra che la
        // pulizia funziona davvero, non solo che è stata "tentata".
        answer_cursor_position_query(&state, &sink);
        wait_until(|| {
            sink.outputs
                .lock()
                .unwrap()
                .iter()
                .any(|b64| decode(b64).contains("continuare"))
        });
        let _ = write(&state, b"\r\n");
        wait_until(|| sink.exited.load(Ordering::SeqCst));
    }
}
