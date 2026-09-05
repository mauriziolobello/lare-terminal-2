"""Registry degli screener del canale financial-markets. Importare questo
package NON ha side-effect pesanti (nessuna rete, nessun file grande) — a
differenza di server.py (refresh ticker, YFinanceDataSource all'import): per
questo la logica di dispatch/validazione vive QUI e non lì, testabile senza
gli side-effect di server.py (vedi test_registry.py).

Aggiungere uno screener futuro: un nuovo modulo in questa cartella (con una
funzione `run(source, tickers, cache_path, discoveries_path, top=25) -> dict`
che ritorna {'summary','report_markdown','channel_summary'[,'title_suffix']})
+ una entry in SCREENERS sotto. Zero tocco a server.py, zero tocco Rust
(a parte l'elenco statico id/titolo nella description del tool run_screener
in external_channel.rs — vedi
Docs/superpowers/specs/2026-08-11-markets-screener-registry-design.md §2)."""

from dataclasses import dataclass
from typing import Callable

from data_sources import format_unavailable_warning
from screeners import citadel, consumer_usage, goldman_sachs, jensen_huang


@dataclass(frozen=True)
class ScreenerDef:
    id: str
    title: str
    description: str
    judgment_instructions: str
    run: Callable[..., dict]


SCREENERS: dict[str, ScreenerDef] = {
    "consumer-usage": ScreenerDef(
        id="consumer-usage",
        title="Uso Consumer",
        description=(
            "Screening 'potenziale inespresso' — aziende consumer USA con score "
            "quantitativo relativo, esclude chi è già apparso negli ultimi 30 giorni."
        ),
        judgment_instructions=(
            "Per OGNI titolo della selezione ricevuta scrivi una riga con: archetipo "
            "d'uso fra quattro etichette fisse (consumer diretto / prodotto fisico / "
            "B2B / incorporato), verdetto Sì/No/Parziale sull'uso di massa reale da "
            "parte del pubblico globale, e una breve motivazione; segnala "
            "esplicitamente i falsi positivi B2B che il pubblico non usa direttamente; "
            "chiudi con un breve avviso che il giudizio è qualitativo e la selezione "
            "non è una raccomandazione operativa. Scrivi la sezione con l'intestazione "
            "'## Giudizio uso di massa'."
        ),
        run=consumer_usage.run,
    ),
    "goldman-sachs": ScreenerDef(
        id="goldman-sachs",
        title="Goldman Sachs",
        description=(
            "Report equity research stile analista senior — top titoli "
            "(default 25), quota minima 30% Technology/Communication Services, bonus "
            "punteggio per prezzo sotto 80-100$, nessuna esclusione "
            "(top assoluto, può ripetersi)."
        ),
        judgment_instructions=(
            "Per OGNI titolo della selezione ricevuta scrivi: vantaggio "
            "competitivo (moat) in breve; risk score 1-10 con spiegazione "
            "(usa il profilo di rischio alto dell'utente solo per "
            "CONTESTUALIZZARE il commento, mai per abbassare il numero — "
            "resta un giudizio oggettivo del titolo); sostenibilità del "
            "dividendo basata su dividend yield/payout ricevuti (se il "
            "titolo non paga dividendi, dillo esplicitamente invece di "
            "forzare un giudizio); zona di ingresso suggerita; stop-loss "
            "suggerito, tarato sull'orizzonte REALE di 6 mesi dell'utente "
            "(il target price a 12 mesi in tabella resta un riferimento "
            "standard separato, non l'orizzonte del suggerimento). Scrivi "
            "la sezione con l'intestazione '## Giudizio equity research'."
        ),
        run=goldman_sachs.run,
    ),
    "jensen-huang": ScreenerDef(
        id="jensen-huang",
        title="Jensen Huang",
        description=(
            "Reverse-engineering dei pick di Jensen Huang (CEO Nvidia) — "
            "fornitori infrastruttura AI (filtro industry) in ipercrescita, "
            "valutazione compressa rispetto alla crescita (PSG), pullback dal "
            "massimo 52 settimane, bonus per legame Nvidia documentato; "
            "nessuna esclusione (top assoluto, può ripetersi)."
        ),
        judgment_instructions=(
            "Per OGNI titolo della selezione ricevuta scrivi: ruolo nella "
            "filiera AI (foundry / memoria-HBM / neocloud / software agentic "
            "/ altro, in base all'industry e a ciò che sai dell'azienda); se "
            "il titolo è marcato 'legame Nvidia: SÌ' spiega in una riga la "
            "natura del legame (investimento Nvidia, citazione in keynote, "
            "partnership), altrimenti dichiara esplicitamente che il legame "
            "non è documentato nella lista curata; sostenibilità "
            "dell'ipercrescita (leggi il PSG: crescita a buon mercato o "
            "già prezzata?); risk score 1-10 con spiegazione (usa il profilo "
            "di rischio alto dell'utente solo per CONTESTUALIZZARE il "
            "commento, mai per abbassare il numero — resta un giudizio "
            "oggettivo del titolo); zona di ingresso suggerita; stop-loss "
            "suggerito, tarato sull'orizzonte REALE di 6 mesi dell'utente. "
            "Chiudi con un breve avviso che il giudizio è qualitativo e la "
            "selezione non è una raccomandazione operativa. Scrivi la "
            "sezione con l'intestazione '## Giudizio ecosistema AI'."
        ),
        run=jensen_huang.run,
    ),
    "citadel": ScreenerDef(
        id="citadel",
        title="Citadel",
        description=(
            "Analisi tecnica quant-style — doppio regime deciso dal trend "
            "recente di ciascun titolo (momentum se il prezzo è sopra la "
            "propria media mobile a 50 giorni, mean-reversion altrimenti), "
            "punteggio a percentili su RSI/MACD/Bollinger/volumi/trend "
            "stack o pullback a seconda del regime, tetto (non quota) di 2 "
            "titoli per settore/industry; nessuna "
            "esclusione (può ripetersi)."
        ),
        judgment_instructions=(
            "Per OGNI titolo della selezione ricevuta scrivi: lettura del "
            "trend sui 3 timeframe ricevuti (giornaliero/settimanale/"
            "mensile) — concordanti o divergenti; pattern grafico "
            "riconoscibile (head & shoulders, cup & handle, doppio "
            "massimo/minimo, triangolo, ecc.) SE presente, dichiarando "
            "esplicitamente se nessun pattern netto è riconoscibile — mai "
            "inventarne uno; lettura del regime assegnato (se momentum: la "
            "forza è sostenibile o già estesa, leggendo RSI/%B ricevuti; se "
            "mean-reversion: ci sono segnali di stabilizzazione — MACD in "
            "miglioramento, volume — o è ancora in caduta libera); zona di "
            "ingresso ANCORATA ai livelli Fibonacci/supporto-resistenza "
            "ricevuti, mai un numero senza riferimento a un livello "
            "calcolato; stop-loss, tarato sull'orizzonte REALE di 6 mesi "
            "dell'utente; target e rapporto rischio/rendimento (R:R), "
            "coerente con la zona d'ingresso e lo stop-loss proposti. "
            "Chiudi con un breve avviso che il giudizio è qualitativo e la "
            "selezione non è una raccomandazione operativa. Scrivi la "
            "sezione con l'intestazione '## Giudizio tecnico'."
        ),
        run=citadel.run,
    ),
}


def dispatch(screener_id: str, source, tickers, cache_path, discoveries_path, top: int = 25) -> dict:
    """Corpo di run_screener (server.py), estratto qui perché importare
    `screeners` non ha side-effect pesanti (a differenza di server.py) —
    testabile senza mock di YFinanceDataSource/ticker cache (vedi
    test_registry.py, che usa data_sources.fake_source.FakeDataSource).
    `screener_id` invalido → dict con SOLO 'error' (mai un'eccezione:
    server.py lo json.dumps() e lo ritorna com'è; l'AI lo vede come testo
    del tool e recupera nel loop tool-error esistente — nessun contratto
    'summary/report_markdown/channel_summary' da rispettare in quel caso)."""
    screener = SCREENERS.get(screener_id)
    if screener is None:
        valid = ", ".join(sorted(SCREENERS))
        return {"error": f"screener_id '{screener_id}' non valido. Id validi: {valid}"}
    result = screener.run(source, tickers, cache_path, discoveries_path, top=top)
    result["title"] = screener.title
    result["summary"] = result["summary"] + "\n\nIstruzioni per il giudizio finale:\n" + screener.judgment_instructions
    # Avviso VISIBILE (stesso principio di report.py::build_report) quando
    # la fonte dichiara campi strutturalmente assenti — in testa al
    # documento del report, prima di qualunque sezione. Nessun avviso per
    # YFinance/FakeDataSource (unavailable_fields() torna set() di default).
    warning = format_unavailable_warning(source.unavailable_fields())
    if warning:
        if "report_markdown" in result:
            result["report_markdown"] = warning + "\n\n" + result["report_markdown"]
        # Fix E (review-fix-wave, 2026-08-14): l'avviso deve arrivare ANCHE
        # a "summary" -- è quello che l'AI legge per il proprio giudizio
        # finale (run_screener, vedi docstring del tool in server.py), a
        # differenza di report.py dove "summary" (per stock_report) deriva
        # da full_markdown, che INCLUDE già l'avviso. Messo in testa (prima
        # delle istruzioni di giudizio già appese sopra): l'AI deve vederlo
        # per primo, prima di leggere i criteri di giudizio.
        result["summary"] = warning + "\n\n" + result["summary"]
    return result


def list_items() -> list[dict]:
    """Vista leggera del registry per list_screeners (server.py) e per il
    picker — solo id/titolo/descrizione, mai la logica di run."""
    return [{"id": s.id, "title": s.title, "description": s.description}
            for s in SCREENERS.values()]
