"""Loop asyncio dedicato, su un thread proprio -- fix per "This event loop
is already running" (vedi task-14-brief.md per la causa confermata:
https://github.com/PrefectHQ/fastmcp/issues/1751, stesso stack FastMCP +
ib_async).

Nessun nome/logica IBKR-specifica qui dentro deliberatamente: questo modulo
e' pura meccanica threading/asyncio, riusabile da qualunque libreria che
richieda "possedere" il proprio event loop (ib_async e' l'unico chiamante
per ora, ma non c'e' nulla che lo leghi a lei). Vive in data_sources/ solo
perche' oggi ha un solo consumer (IbkrDataSource) -- un Task futuro puo'
spostarlo altrove senza cambiarne il contenuto.

Perche' un thread dedicato: l'API sincrona di ib_async (IB.connect()) prova
a possedere/eseguire un proprio event loop asyncio internamente. Se il
processo che la chiama ha GIA' un loop attivo sul thread corrente (il caso
di FastMCP, che serve le richieste MCP dentro il proprio loop async), i due
collidono. La soluzione (identica nello spirito al pattern ufficiale
dell'API nativa IBKR, `threading.Thread(target=app.run).start()`) e'
eseguire ib_async su un thread SEPARATO che possiede un loop TUTTO SUO --
mai condiviso col thread chiamante. `run_coroutine_threadsafe` e' il ponte
sicuro fra i due thread: sottomette una coroutine al loop dedicato da
QUALSIASI altro thread e ritorna un `concurrent.futures.Future` (non un
`asyncio.Future`) su cui il thread chiamante puo' bloccarsi con `.result()`
in modo normale, senza bisogno del proprio loop."""

import asyncio
import threading
from concurrent.futures import TimeoutError as FutureTimeoutError

# Singleton lazy: None finche' nessuno chiama run_on_ibkr_loop. Un tool
# Python che non tocca mai IBKR non paga il costo di un thread sempre
# acceso. _start_lock protegge SOLO la creazione/avvio (una manciata di
# istruzioni) -- non l'esecuzione delle coroutine, che gira libera sul loop
# dedicato una volta partito (vedi run_on_ibkr_loop sotto e il test
# "concurrent calls without deadlock").
_loop: asyncio.AbstractEventLoop | None = None
_thread: threading.Thread | None = None
_start_lock = threading.Lock()


def _run_forever(loop: asyncio.AbstractEventLoop) -> None:
    """Funzione target del thread dedicato. `asyncio.set_event_loop` rende
    `loop` il loop "corrente" per QUESTO thread (ogni thread ha il proprio
    concetto di loop corrente in asyncio) -- poi `run_forever()` blocca
    questo thread per sempre, processando i task che gli vengono sottomessi
    da altri thread via run_coroutine_threadsafe."""
    asyncio.set_event_loop(loop)
    loop.run_forever()


def _ensure_loop_started() -> asyncio.AbstractEventLoop:
    """Avvia il thread dedicato alla prima chiamata (lazy), thread-safe:
    due chiamate concorrenti alla prima richiesta non devono avviare due
    thread/loop distinti -- solo la PRIMA vince la corsa, le successive
    trovano `_loop` gia' valorizzato e ritornano subito."""
    global _loop, _thread
    with _start_lock:
        if _loop is None:
            _loop = asyncio.new_event_loop()
            # daemon=True: questo thread non deve impedire la chiusura del
            # processo Python -- nessuno lo "spegne" mai esplicitamente,
            # vive quanto il processo (stesso principio della sessione
            # shell persistente in mcp-server, ma qui e' Python non Rust).
            _thread = threading.Thread(target=_run_forever, args=(_loop,), daemon=True)
            _thread.start()
        return _loop


def run_on_ibkr_loop(coro_factory, timeout: float = 60.0):
    """Esegue `coro_factory()` (funzione zero-argomenti che RITORNA una
    coroutine, mai una coroutine gia' creata -- crearla qui dentro, non
    prima della chiamata, evita il warning "coroutine was never awaited" se
    run_coroutine_threadsafe fallisse a sottomettere) sul loop dedicato,
    bloccando il thread CHIAMANTE (quello di FastMCP) fino al risultato o
    al timeout. Ogni chiamata a ib_async deve passare da qui -- mai una
    coroutine awaitata direttamente nel loop del chiamante (e' esattamente
    quello che causerebbe di nuovo "This event loop is already running").

    Solleva `TimeoutError` (messaggio leggibile, non l'eccezione grezza di
    concurrent.futures) se la coroutine non completa entro `timeout`
    secondi. Qualunque altra eccezione sollevata dalla coroutine si
    propaga al chiamante invariata (mai ingoiata o trasformata)."""
    loop = _ensure_loop_started()
    future = asyncio.run_coroutine_threadsafe(coro_factory(), loop)
    try:
        return future.result(timeout=timeout)
    except FutureTimeoutError as e:
        raise TimeoutError(
            f"Operazione IBKR non completata entro {timeout}s (loop dedicato)."
        ) from e
