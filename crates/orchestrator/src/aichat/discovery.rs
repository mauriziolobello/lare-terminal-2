//! Scoperta dei peer. `PeerTable` è la parte PURA: tiene chi è stato visto e quando,
//! e fa decadere chi non si annuncia più da oltre il TTL. L'orologio è iniettato
//! (`now_ms`) → test deterministici, niente clock reale.
//! Il `Discoverer` reale (UDP) arriva in Task 10; qui solo logica + (Task 6) il seam.

use crate::aichat::peer::{PeerId, PeerInfo, Roster};
use async_trait::async_trait;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Tabella dei peer visti di recente, con scadenza per TTL.
///
/// # Principio di funzionamento
/// Ogni volta che riceviamo un annuncio UDP da un peer (in futuro, via `Discoverer`)
/// chiamiamo `observe(info, now_ms)`: la tabella registra l'id e il timestamp.
/// Periodicamente chiamiamo `expire(now_ms)`: i peer il cui ultimo avvistamento è
/// più vecchio del TTL vengono rimossi e restituiti come "decaduti" (il chiamante
/// può propagare l'uscita al resto del sistema — es. rielezionare il server).
///
/// # Iniezione del clock
/// `now_ms: u64` è passato esplicitamente invece di leggere `SystemTime::now()`:
/// → i test usano timestamps inventati → zero I/O → esecuzione istantanea + deterministica.
pub struct PeerTable {
    ttl_ms: u64,
    /// id → (info, istante dell'ultimo avvistamento in ms).
    seen: HashMap<PeerId, (PeerInfo, u64)>,
}

impl PeerTable {
    /// Crea una nuova tabella con il Time-To-Live indicato in millisecondi.
    pub fn new(ttl_ms: u64) -> Self {
        Self { ttl_ms, seen: HashMap::new() }
    }

    /// Registra un annuncio ricevuto a `now_ms` (aggiorna l'ultimo avvistamento).
    ///
    /// Se il peer era già presente, sovrascrive le info e aggiorna il timestamp:
    /// questo è il meccanismo di "rinnovo" che impedisce la scadenza di peer attivi.
    pub fn observe(&mut self, info: PeerInfo, now_ms: u64) {
        // `insert` è idempotente per la stessa chiave: sovrascrive silenziosamente.
        self.seen.insert(info.id, (info, now_ms));
    }

    /// Rimuove i peer non più visti da oltre il TTL. Ritorna gli id decaduti.
    ///
    /// Un peer decade se: `now_ms - last_seen_ms > ttl_ms`.
    /// `saturating_sub` evita underflow in caso di bug nel clock (e.g. now < last).
    pub fn expire(&mut self, now_ms: u64) -> Vec<PeerId> {
        let ttl = self.ttl_ms;
        // Prima raccogliamo gli id da rimuovere (non possiamo mutare `seen`
        // mentre lo iteriamo — borrow checker Rust).
        let stale: Vec<PeerId> = self
            .seen
            .iter()
            .filter(|(_, (_, last))| now_ms.saturating_sub(*last) > ttl)
            .map(|(id, _)| *id)
            .collect();
        for id in &stale {
            self.seen.remove(id);
        }
        stale
    }

    /// Snapshot del roster corrente (membri vivi).
    ///
    /// Costruisce un `Roster` nuovo: `Roster::upsert` è idempotente sull'id,
    /// quindi l'ordine di inserimento non conta.
    pub fn roster(&self) -> Roster {
        let mut r = Roster::new();
        for (info, _) in self.seen.values() {
            r.upsert(info.clone());
        }
        r
    }
}

/// Seam di scoperta: annuncia la propria presenza e fornisce gli annunci ricevuti.
/// In produzione: `UdpDiscoverer` (Task 10). Nei test: `FakeDiscoverer`.
///
/// Firma estesa (Bug 2, late-joiner): sia `announce` che `next` ora portano anche il
/// leader creduto — il nostro (in uscita) e quello riportato dal mittente (in entrata).
/// Serve a propagare l'informazione "un server esiste già altrove" ai peer che si
/// uniscono al gruppo dopo la sua elezione — vedi `election::elect` e
/// `AiChatService::reported_leader`.
///
/// # `&self` su ENTRAMBI i metodi (debito #6, "discovery full-duplex")
/// `next` prendeva `&mut self` (serviva al buffer riusabile di `UdpDiscoverer`), mentre
/// `announce` prende `&self`: le due firme non potevano coesistere in un `tokio::select!`
/// sulla STESSA variabile `disc` — Rust vieta un prestito `&mut` e uno `&` simultanei sullo
/// stesso valore (aliasing). Risultato pratico: il chiamante era costretto a un loop
/// SEQUENZIALE (annuncia, poi ascolta per una finestra, poi ri-annuncia), niente ascolto
/// continuo mentre si annuncia. Con `next(&self)` (implementazioni: buffer di ricezione
/// ALLOCATO LOCALMENTE ad ogni chiamata in `UdpDiscoverer`, lock interno in
/// `FakeDiscoverer`) entrambi i metodi si possono chiamare da un `&disc` condiviso,
/// abilitando un vero `select!` full-duplex nel chiamante (vedi `main.rs`).
#[async_trait]
pub trait Discoverer: Send {
    /// Manda un annuncio di presenza (UDP broadcast in produzione).
    /// `leader`: il leader attualmente creduto da NOI, da incorporare nell'annuncio.
    async fn announce(&self, leader: Option<PeerId>) -> std::io::Result<()>;
    /// Prossimo annuncio ricevuto da un altro peer; `None` = sorgente esaurita/chiusa.
    /// Ritorna anche il leader che il MITTENTE ha riportato di credere valido.
    async fn next(&self) -> Option<(PeerInfo, Option<PeerId>)>;
}

/// Fake in-memory: restituisce annunci prefissati e conta le `announce()`.
///
/// Utile nei test: permette di iniettare `(PeerInfo, Option<PeerId>)` sintetici e
/// verificare quante volte il codice in esame ha invocato `announce()`, e con quale
/// ultimo leader.
pub struct FakeDiscoverer {
    /// Coda degli annunci da restituire in sequenza via `next()`.
    /// `Mutex` invece di un campo semplice: `next()` ora prende `&self` (debito #6,
    /// "discovery full-duplex" — vedi doc-comment del trait `Discoverer`), quindi non può
    /// più mutare `incoming` con un prestito esclusivo. Il `Mutex` fornisce la mutabilità
    /// interna: il lock si prende, si fa `pop_front`, si rilascia — tutto sincrono, nessun
    /// `.await` tenuto col lock aperto (nessun rischio di deadlock/contesa reale nei test).
    incoming: std::sync::Mutex<VecDeque<(PeerInfo, Option<PeerId>)>>,
    /// Contatore atomico delle chiamate a `announce()`.
    /// `AtomicUsize` permette di leggere il contatore da `&self` (senza `mut`)
    /// anche mentre `&mut self` è in prestito per `next()`.
    announces: AtomicUsize,
    /// Ultimo `leader` passato ad `announce()`, per assertion nei test.
    /// `Mutex<Option<PeerId>>` invece di un `Atomic*` perché `PeerId` non è un
    /// intero primitivo; il lock è a brevissima durata (solo lettura/scrittura
    /// dell'`Option`), nessun rischio di contesa reale nei test.
    last_announced_leader: std::sync::Mutex<Option<PeerId>>,
}

impl FakeDiscoverer {
    /// Crea un nuovo fake con la lista di `(PeerInfo, Option<PeerId>)` da restituire
    /// in sequenza via `next()`.
    pub fn new(incoming: Vec<(PeerInfo, Option<PeerId>)>) -> Self {
        Self {
            incoming: std::sync::Mutex::new(incoming.into()),
            announces: AtomicUsize::new(0),
            last_announced_leader: std::sync::Mutex::new(None),
        }
    }

    /// Restituisce quante volte `announce()` è stata chiamata sul fake.
    pub fn announce_count(&self) -> usize {
        self.announces.load(Ordering::Relaxed)
    }

    /// Restituisce l'ultimo `leader` passato ad `announce()` (o `None` se non ancora
    /// chiamata, o se l'ultima chiamata aveva `leader: None`).
    pub fn last_announced_leader(&self) -> Option<PeerId> {
        *self.last_announced_leader.lock().unwrap()
    }
}

#[async_trait]
impl Discoverer for FakeDiscoverer {
    async fn announce(&self, leader: Option<PeerId>) -> std::io::Result<()> {
        // Incrementa il contatore in modo thread-safe e registra il leader passato.
        self.announces.fetch_add(1, Ordering::Relaxed);
        *self.last_announced_leader.lock().unwrap() = leader;
        Ok(())
    }

    async fn next(&self) -> Option<(PeerInfo, Option<PeerId>)> {
        // Prende il lock, estrae l'elemento in testa alla coda, rilascia il lock.
        // `unwrap()`: un lock avvelenato (panic di un altro thread mentre lo teneva)
        // non ha un fallback sensato nei test — meglio propagare il panic che
        // proseguire con uno stato incoerente.
        self.incoming.lock().unwrap().pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn info(d: u8) -> PeerInfo {
        PeerInfo { id: PeerId(Ipv4Addr::new(192, 168, 1, d)), label_base: "x".into(), chat_port: 40100 }
    }

    #[test]
    fn observe_adds_to_roster() {
        let mut t = PeerTable::new(1000);
        t.observe(info(10), 0);
        t.observe(info(20), 0);
        assert_eq!(t.roster().len(), 2);
    }

    #[test]
    fn re_observe_refreshes_last_seen_and_does_not_duplicate() {
        let mut t = PeerTable::new(1000);
        t.observe(info(10), 0);
        t.observe(info(10), 500); // stesso id, più tardi
        assert_eq!(t.roster().len(), 1);
        // a t=1200 NON deve decadere: ultimo avvistamento a 500, TTL 1000 → vivo fino a 1500.
        assert!(t.expire(1200).is_empty());
        assert_eq!(t.roster().len(), 1);
    }

    #[test]
    fn expire_removes_stale_peers_and_returns_them() {
        let mut t = PeerTable::new(1000);
        t.observe(info(10), 0);
        t.observe(info(20), 800);
        // a t=1500: .10 (visto a 0) è scaduto (>1000 fa), .20 (visto a 800) no.
        let expired = t.expire(1500);
        assert_eq!(expired, vec![PeerId(Ipv4Addr::new(192, 168, 1, 10))]);
        assert_eq!(t.roster().len(), 1);
        assert!(t.roster().contains(PeerId(Ipv4Addr::new(192, 168, 1, 20))));
    }

    #[tokio::test]
    async fn fake_discoverer_yields_prefixed_then_none() {
        // `next()` ritorna (PeerInfo, Option<PeerId>): il secondo elemento è il leader
        // che quel peer ha riportato di credere valido (Bug 2, late-joiner).
        // `d` non serve più `mut`: `next(&self)` muta `incoming` tramite il `Mutex`
        // interno (mutabilità interna), non tramite un prestito esclusivo del binding.
        let d = FakeDiscoverer::new(vec![(info(10), None), (info(20), Some(info(10).id))]);
        assert_eq!(d.next().await.map(|(i, l)| (i.id, l)), Some((info(10).id, None)));
        assert_eq!(d.next().await.map(|(i, l)| (i.id, l)), Some((info(20).id, Some(info(10).id))));
        assert!(d.next().await.is_none()); // coda esaurita
    }

    // -------------------------------------------------------------------------
    // Debito #6 ("discovery full-duplex"): prova che `announce(&self)` e `next(&self)`
    // si possono usare ENTRAMBE su un `&d` condiviso — vedi doc-comment di `Discoverer`.
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn announce_and_next_usable_concurrently_on_shared_ref() {
        // Prova concettuale a livello di TIPO: prima del fix, `next` prendeva `&mut self`
        // mentre `announce` prendeva `&self` — le due chiamate non potevano coesistere su
        // uno stesso `&d` (il borrow checker rifiuterebbe un prestito `&mut` mentre esiste
        // un prestito `&` attivo). Qui costruiamo ENTRAMBI i future da un singolo prestito
        // immutabile `d_ref` e li eseguiamo insieme con `tokio::join!` (che tiene vivi
        // entrambi i prestiti attraverso i rispettivi punti di sospensione, esattamente
        // come farebbe un `tokio::select!` in `main.rs`). Se `next` richiedesse ancora
        // `&mut self`, questo modulo non compilerebbe (E0502) — il fatto che compili ED
        // esegua è la dimostrazione che l'aliasing `&mut`/`&` è sparito.
        let d = FakeDiscoverer::new(vec![(info(10), None)]);
        let d_ref = &d;
        let (announce_res, next_res) = tokio::join!(d_ref.announce(None), d_ref.next());
        assert!(announce_res.is_ok());
        assert_eq!(next_res.map(|(i, l)| (i.id, l)), Some((info(10).id, None)));
    }

    #[tokio::test]
    async fn fake_discoverer_records_announce_count() {
        let d = FakeDiscoverer::new(vec![]);
        d.announce(None).await.unwrap();
        d.announce(None).await.unwrap();
        assert_eq!(d.announce_count(), 2);
    }

    // -------------------------------------------------------------------------
    // Bug 2 (late-joiner): `announce(leader)` registra l'ultimo leader passato —
    // vedi Docs/superpowers/specs/2026-07-02-aichat-late-joiner-election-design.md §3.5
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn fake_discoverer_records_last_announced_leader() {
        let d = FakeDiscoverer::new(vec![]);
        d.announce(None).await.unwrap();
        d.announce(Some(info(10).id)).await.unwrap();
        assert_eq!(d.last_announced_leader(), Some(info(10).id));
    }
}
