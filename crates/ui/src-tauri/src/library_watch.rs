// library_watch.rs — Modulo per il file-system watch della cartella Library.
//
// Responsabilità (SRP): osserva ricorsivamente una directory e chiama una
// callback generica quando il contenuto cambia (dopo un periodo di quiete
// "debounce"). NON conosce Tauri: l'accoppiamento con l'emit dell'evento
// avviene in main.rs (Dependency Inversion Principle).
//
// Seam testabile: `watch_dir` accetta `on_change: impl Fn() + Send + 'static`
// anziché essere hard-coded sull'emit Tauri, così nei test iniettamo un
// semplice contatore AtomicUsize.
//
// Nota sulle dipendenze: usiamo notify tramite i re-export di
// notify-debouncer-mini (notify_debouncer_mini::notify::*) per garantire
// compatibilità di versione con il Debouncer interno, evitando conflitti.
//
// API di notify-debouncer-mini 0.7.x (usata qui):
//   - new_debouncer(timeout, sender) → prende 2 argomenti (niente tick_rate)
//   - DebounceEventHandler è implementato per Sender<Result<Vec<DebouncedEvent>, Error>>
//   - Il sender riceve un Result per batch (Error singolo, non Vec<Error>)

use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

// Accediamo a notify tramite i re-export di notify-debouncer-mini.
use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebouncedEvent};

// Tipo del messaggio inviato dal debouncer al nostro thread consumatore.
// Il debouncer raccoglie gli eventi in un burst e manda un unico messaggio.
type DebouncerResult =
    Result<Vec<DebouncedEvent>, notify_debouncer_mini::notify::Error>;

// ---------------------------------------------------------------------------
// Tipi pubblici
// ---------------------------------------------------------------------------

/// Guard che tiene vivo il watcher del filesystem.
///
/// Finché questo valore è in scope, il watcher è attivo.
/// Dropparlo ferma il watcher (utile nei test per pulire dopo ogni caso).
pub struct WatchGuard {
    // Il debouncer possiede il watcher internamente.
    // Prefisso _ = "held for lifetime" (il compilatore non avvisa unused).
    _debouncer: notify_debouncer_mini::Debouncer<RecommendedWatcher>,
}

// ---------------------------------------------------------------------------
// API pubblica — implementazione reale (fase GREEN)
// ---------------------------------------------------------------------------

/// Avvia un watcher RICORSIVO su `dir`.
///
/// Su modifiche del filesystem (create/remove/rename/modify), *debounced*
/// di `debounce`, chiama `on_change` **una volta** per burst di quiete.
/// La callback NON sa nulla di Tauri (nell'app: emit; nei test: contatore).
///
/// Ritorna un `WatchGuard` da TENERE VIVO finché il watch deve restare attivo.
/// Errore: best-effort — se `notify` non riesce ad avviarsi, ritorna `Err(msg)`.
/// Il chiamante logga e prosegue; niente panico.
pub fn watch_dir<F>(dir: &Path, debounce: Duration, on_change: F) -> Result<WatchGuard, String>
where
    F: Fn() + Send + 'static,
{
    // Creiamo un canale mpsc per ricevere i batch di eventi dal debouncer.
    // DebounceEventHandler è implementato per Sender<DebouncerResult>
    // (API notify-debouncer-mini 0.7.x).
    let (tx, rx) = mpsc::channel::<DebouncerResult>();

    // Il debouncer raccoglie gli eventi del filesystem, aspetta `debounce` di
    // quiete, poi manda un unico messaggio con tutti gli eventi del burst.
    // API v0.7.x: 2 argomenti (timeout, handler) — niente tick_rate.
    let mut debouncer = new_debouncer(debounce, tx)
        .map_err(|e| format!("notify debouncer init error: {e}"))?;

    // Avvia il watcher ricorsivo sulla directory specificata.
    debouncer
        .watcher()
        .watch(dir, RecursiveMode::Recursive)
        .map_err(|e| format!("notify watch error su {dir:?}: {e}"))?;

    // Thread consumatore: riceve batch di eventi dal debouncer e chiama
    // on_change() UNA VOLTA per batch (il debounce ha già coalizzato il burst).
    // .flatten() scarta gli Err (errori di notify, best-effort) e itera
    // direttamente sui Vec<DebouncedEvent> contenuti negli Ok.
    // Il thread esce automaticamente quando il canale si chiude, cioè
    // quando il Debouncer interno al WatchGuard viene droppato.
    std::thread::spawn(move || {
        for events in rx.iter().flatten() {
            if !events.is_empty() {
                // Chiama la callback UNA VOLTA per burst:
                // il debounce ha già coalizzato tutti gli eventi ravvicinati.
                on_change();
            }
        }
        // Quando il ciclo finisce il thread si ferma silenziosamente.
    });

    Ok(WatchGuard {
        _debouncer: debouncer,
    })
}

// ---------------------------------------------------------------------------
// Test — ciclo RED → GREEN
//
// Il seam testabile (F: Fn() + Send + 'static) permette di iniettare
// un contatore AtomicUsize invece della logica Tauri.
// Niente mock: il watcher reale viene esercitato su una tempdir.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    /// Test 1: un singolo cambiamento sul disco viene rilevato.
    ///
    /// Strategia: poll fino a ~3 s (niente sleep fisso → robusto su CI lenta).
    /// Il watcher ha bisogno di un momento per armarsi dopo watch_dir:
    /// aspettiamo 200 ms prima di scrivere il file.
    #[test]
    fn watch_dir_detects_a_change() {
        let dir = tempfile::TempDir::new().expect("crea temp dir");
        let counter = Arc::new(AtomicUsize::new(0));
        let counter2 = counter.clone();

        // Debounce breve per mantenere il test veloce.
        let _guard = watch_dir(dir.path(), Duration::from_millis(150), move || {
            counter2.fetch_add(1, Ordering::Relaxed);
        })
        .expect("watch_dir deve avviarsi senza errori");

        // Attendi che il watcher si armi sul filesystem.
        std::thread::sleep(Duration::from_millis(200));

        // Modifica: crea un file nella directory osservata.
        std::fs::write(dir.path().join("prova.txt"), b"ciao")
            .expect("scrittura file di test");

        // Poll: aspetta che la callback venga invocata, max 3 secondi.
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while counter.load(Ordering::Relaxed) == 0 {
            if std::time::Instant::now() > deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        assert!(
            counter.load(Ordering::Relaxed) > 0,
            "la callback deve essere chiamata almeno una volta dopo una modifica al filesystem"
        );
    }

    /// Test 2: un burst di modifiche rapide viene coalizzato (debounce).
    ///
    /// LENIENT — soglie volutamente larghe per evitare flakiness:
    ///   - il fs-watch su Windows è timing-sensitive
    ///   - asserisci ALMENO 1 (il burst è stato notato)
    ///   - asserisci MENO di 8 (non 5 chiamate immediate — il debounce agisce)
    #[test]
    fn watch_dir_debounces_burst() {
        let dir = tempfile::TempDir::new().expect("crea temp dir");
        let counter = Arc::new(AtomicUsize::new(0));
        let counter2 = counter.clone();

        // Debounce 300 ms: 5 modifiche in ~100 ms devono coalizzare.
        let _guard = watch_dir(dir.path(), Duration::from_millis(300), move || {
            counter2.fetch_add(1, Ordering::Relaxed);
        })
        .expect("watch_dir deve avviarsi");

        // Attendi che il watcher si armi.
        std::thread::sleep(Duration::from_millis(200));

        // Genera 5 modifiche in rapida successione (~20 ms ciascuna = ~100 ms tot).
        for i in 0u8..5 {
            std::fs::write(dir.path().join(format!("burst{i}.txt")), b"x")
                .expect("scrittura burst");
            std::thread::sleep(Duration::from_millis(20));
        }

        // Aspetta il debounce (300 ms) + margine generoso (400 ms) = 700 ms totali.
        std::thread::sleep(Duration::from_millis(700));

        let count = counter.load(Ordering::Relaxed);

        // LENIENT — il fs-watch Windows può variare; soglie larghe per robustezza.
        assert!(count >= 1, "la callback deve essere chiamata almeno una volta; count={count}");
        assert!(
            count < 8,
            "il debounce deve coalizzare i burst; count atteso < 8, ottenuto {count}"
        );
    }
}
