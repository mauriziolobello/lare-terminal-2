//! # logging — inizializzazione di `tracing`, panic-free (fix round 1, D6)
//!
//! `main()` chiama SOLO [`init_logging`], una volta sola all'avvio. Ogni
//! altro punto del crate chiamante usa le macro `tracing::info!`/`warn!`/
//! `error!` senza sapere nulla di come/dove finiscano i log — stesso
//! principio di incapsulamento di [`crate::runtime_config::RuntimeConfig`]
//! nell'orchestrator: una decisione presa in un solo punto, non ri-derivata
//! altrove.
//!
//! ## Perché questo modulo esiste (fix round 1)
//! La prima versione di Task 4 (nell'orchestrator) costruiva il file di log
//! con `tracing_appender::rolling::daily(dir, prefix)`. Quella funzione, se la
//! cartella `dir` non è creabile o non è scrivibile (permessi negati,
//! percorso occupato da un file, disco pieno, ...), fa `.expect(...)`
//! internamente e va in **panic** — un demone pensato per girare inosservato
//! in autostart si sarebbe fermato subito con un crash che nessuno vede
//! (nessun terminale, senza `--console-log`). Ogni altra sorgente di
//! configurazione di questo task (token, llms.json, telegramsettings.json,
//! mcp-server path, ...) fallisce in modo morbido con un fallback — questo
//! era l'unico punto rimasto che poteva ancora panicare, ed è quello che
//! questo modulo elimina.
//!
//! La soluzione: [`open_log_file`] è una funzione pura (non tocca mai il
//! subscriber globale di `tracing`, quindi è testabile in isolamento e più
//! volte nello stesso processo di test) che prova a preparare il file di log
//! e ritorna `Result<_, String>` invece di andare in panic. [`init_logging`]
//! la usa e, se fallisce, stampa il motivo su stderr con `eprintln!` e
//! installa un subscriber di sola console — un demone rumoroso è comunque
//! meglio di un demone che sparisce senza lasciare traccia.
//!
//! ## Perché è qui, in `startup-config`, e non in `orchestrator`
//! Il modulo nasce nell'orchestrator (unico consumatore all'epoca) e viene
//! spostato qui perché `ui.exe` ne ha bisogno identico (stesso problema:
//! nessuna console in release, quindi nessun posto dove far atterrare
//! `println!`/`eprintln!`) — `startup-config` è già dipendenza di entrambi.
//! Il nome del file di log (`"orchestrator.log"` prima, hardcoded) diventa un
//! parametro (`filename_prefix`) così ogni chiamante ottiene un file proprio
//! (`orchestrator.log.<data>`, `ui.log.<data>`, ...) nella stessa cartella
//! `Configuration/logs/`.

use std::path::Path;

use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;

/// "Ricevuta" dell'inizializzazione del logging, da tenere viva per tutta la
/// durata di `main()` (esattamente come il precedente `_guard` locale).
///
/// Il campo è un `Option` perché nel ramo di fallback (file di log non
/// disponibile) non esiste nessun `WorkerGuard` da tenere in vita — non c'è
/// nulla da flushare oltre a stderr, che è sincrono. Il campo non viene mai
/// letto esplicitamente: la sua unica funzione è vivere abbastanza a lungo
/// da non far droppare anticipatamente il `WorkerGuard` che contiene (droppare
/// quel guard interrompe il flush del writer non bloccante del file di log)
/// — da cui l'`#[allow(dead_code)]`, onesto sul perché il campo "non viene
/// letto" pur essendo indispensabile.
#[allow(dead_code)]
pub struct LoggingGuard(Option<WorkerGuard>);

/// Prepara (creando la cartella se serve) il file di log giornaliero
/// `<log_dir>/<filename_prefix>.<data>`, SENZA mai andare in panic.
///
/// Pura rispetto al subscriber globale di `tracing`: non chiama mai
/// `.init()`, quindi può essere invocata liberamente (anche più volte) nei
/// test, cosa che [`init_logging`] — che installa un subscriber di processo
/// una tantum — non permetterebbe.
///
/// Ogni fallimento (cartella non creabile, percorso occupato da un file,
/// permessi negati, ...) diventa un `Err(String)` leggibile, mai un panic.
pub fn open_log_file(
    log_dir: &Path,
    filename_prefix: &str,
) -> Result<(NonBlocking, WorkerGuard), String> {
    // `create_dir_all` su una cartella già esistente non è un errore (idempotente) —
    // è la stessa chiamata che la v1 di questo task faceva con `let _ = ...`,
    // qui però l'esito viene osservato invece di essere scartato.
    std::fs::create_dir_all(log_dir).map_err(|e| {
        format!(
            "impossibile creare la cartella di log {}: {e}",
            log_dir.display()
        )
    })?;
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(filename_prefix)
        // A differenza di `tracing_appender::rolling::daily(...)`, che internamente fa
        // `.expect(...)` e va in panic sull'errore, `Builder::build` ritorna un `Result`
        // esplicito — questo è l'unico cambiamento che elimina il panic del fix round 1.
        .build(log_dir)
        .map_err(|e| {
            format!(
                "impossibile aprire il file di log in {}: {e}",
                log_dir.display()
            )
        })?;
    Ok(tracing_appender::non_blocking(appender))
}

/// Inizializza il subscriber globale di `tracing` per l'intero processo.
/// Va chiamata UNA sola volta (da `main()`), mai due volte nello stesso
/// processo — un secondo `.init()` andrebbe comunque in panic (limite di
/// `tracing_subscriber`, non di questo modulo), motivo per cui questa
/// funzione non è pensata per l'uso nei test (che infatti esercitano solo
/// [`open_log_file`], pura rispetto al subscriber globale).
///
/// - File di log sempre attivo quando `open_log_file` riesce; su stderr in
///   più solo se `console` (usato da `init_*.ps1` per il debug interattivo —
///   `ui.exe`, che non ha mai una console, chiama sempre con `console: false`).
/// - Se `open_log_file` fallisce: fallback su stderr SEMPRE (indipendentemente
///   da `console`) — un demone senza alcun log è peggio di uno rumoroso.
/// - `level`: stessa stringa di `startup.json` (`rt.startup.log.level` /
///   `cfg_state.startup.log.level`), parsata con fallback a `INFO` se
///   assente/non valida (invariato dalla v1 di questo task).
pub fn init_logging(
    log_dir: &Path,
    filename_prefix: &str,
    level: &str,
    console: bool,
) -> LoggingGuard {
    let level = level
        .parse::<tracing::Level>()
        .unwrap_or(tracing::Level::INFO);
    match open_log_file(log_dir, filename_prefix) {
        Ok((file_writer, guard)) => {
            let file_layer = fmt::layer().with_writer(file_writer).with_ansi(false);
            let registry = tracing_subscriber::registry()
                .with(tracing_subscriber::filter::LevelFilter::from_level(level))
                .with(file_layer);
            if console {
                registry
                    .with(fmt::layer().with_writer(std::io::stderr))
                    .init();
            } else {
                registry.init();
            }
            LoggingGuard(Some(guard))
        }
        Err(e) => {
            // `eprintln!` e non `tracing::error!`: il subscriber non è ancora installato,
            // quindi qualunque macro di tracing andrebbe persa (nessun sink attivo).
            eprintln!("log su file non disponibile ({e}) — fallback sulla sola console");
            tracing_subscriber::registry()
                .with(tracing_subscriber::filter::LevelFilter::from_level(level))
                .with(fmt::layer().with_writer(std::io::stderr))
                .init();
            LoggingGuard(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Caso felice: cartella non ancora esistente → `open_log_file` la crea e
    /// ritorna `Ok`. Copre lo stesso percorso che `main()` esercita a ogni
    /// avvio normale (prima esecuzione: `Configuration/logs/` non c'è ancora).
    #[test]
    fn open_log_file_su_cartella_non_esistente_la_crea_e_ritorna_ok() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let log_dir = tmp.path().join("logs");
        assert!(
            !log_dir.exists(),
            "precondizione: la cartella non deve esistere ancora"
        );

        let result = open_log_file(&log_dir, "test.log");

        assert!(result.is_ok(), "atteso Ok, trovato Err: {:?}", result.err());
        assert!(
            log_dir.exists(),
            "open_log_file deve creare la cartella di log"
        );
    }

    /// Caso di regressione del fix round 1: il "genitore" del percorso di log
    /// è un FILE regolare, non una cartella — `create_dir_all` non può creare
    /// `blocker/logs` perché `blocker` non è attraversabile come directory.
    /// Prima del fix questo scenario mandava in panic l'intero processo
    /// chiamante all'avvio; ora deve limitarsi a un `Err` leggibile.
    #[test]
    fn open_log_file_con_genitore_che_e_un_file_ritorna_err_senza_panic() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let blocker = tmp.path().join("blocker");
        std::fs::write(&blocker, b"non sono una cartella").expect("scrittura file blocker");
        let log_dir = blocker.join("logs");

        let result = open_log_file(&log_dir, "test.log");

        assert!(
            result.is_err(),
            "atteso Err quando il genitore del percorso di log e' un file regolare, trovato Ok"
        );
    }

    /// Il prefisso passato va davvero usato nel nome del file — nessun altro
    /// test qui sopra lo verifica (solo che *un* file venga creato, non
    /// *quale* nome), ed è proprio ciò che rende questo modulo riusabile da
    /// più chiamanti (`orchestrator.log`, `ui.log`, ...) senza collisioni.
    #[test]
    fn open_log_file_usa_il_prefisso_passato_nel_nome_del_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let log_dir = tmp.path().join("logs");

        let result = open_log_file(&log_dir, "ui.log");
        assert!(result.is_ok(), "atteso Ok: {:?}", result.err());

        // `tracing_appender` con rotazione DAILY nomina il file
        // "<prefix>.<YYYY-MM-DD>" — basta verificare che ESISTA un file il cui
        // nome comincia per "ui.log" (non un nome fisso, dipende dalla data).
        let found = std::fs::read_dir(&log_dir)
            .expect("la cartella deve esistere")
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().starts_with("ui.log"));
        assert!(found, "atteso un file che inizia per 'ui.log' in {log_dir:?}");
    }
}
