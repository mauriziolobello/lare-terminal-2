"""Server MCP del dominio financial-markets — quattro tool fissi (search_ticker,
stock_report, list_stocks) più un dispatcher generico per screener multipli
(list_screeners/run_screener, registry in screeners/). Vedi
Docs/superpowers/specs/2026-07-22-financial-markets-stock-report-design.md,
Docs/superpowers/specs/2026-07-24-financial-markets-list-stocks-design.md e
Docs/superpowers/specs/2026-08-11-markets-screener-registry-design.md.
"""

import json
from pathlib import Path

from mcp.server.fastmcp import FastMCP

import refresh_tickers
import report
import screeners
import stock_list
import ticker_search
import local_dir
import market_data_config

mcp = FastMCP("financial-markets")

TICKERS_PATH = Path(__file__).parent / "tickers_us.json"
FUNDAMENTALS_CACHE_PATH = Path(__file__).parent / "fundamentals_cache.json"
DISCOVERIES_PATH = Path(__file__).parent / "discoveries.json"

# Refresh automatico su staleness (7 giorni) — una chiamata di rete
# occasionale all'avvio, non ad ogni singola ricerca.
if refresh_tickers.is_stale(TICKERS_PATH):
    refresh_tickers.refresh(TICKERS_PATH)

_source = market_data_config.build_data_source(local_dir.resolve() / "market_data.json")

# Caricata una sola volta all'avvio del processo (come _source sopra) — non
# ad ogni chiamata di search_ticker: il file è ~750KB/~10.400 righe, e
# ricaricarlo/riparsarlo ad ogni tool invocation è spreco puro dato che il
# refresh su staleness (sopra) è già l'unico punto in cui la cache può
# cambiare durante la vita del processo.
_tickers = ticker_search.load_tickers(TICKERS_PATH)


@mcp.tool()
def search_ticker(query: str) -> list[dict]:
    """Match parziale case-insensitive su nome o ticker nella cache locale
    USA. Ritorna [{'ticker','name'}, ...], vuoto se nessun match. Usalo
    quando l'utente scrive un nome societario invece di un ticker."""
    return ticker_search.search(query, _tickers)


@mcp.tool()
def stock_report(ticker: str) -> str:
    """Report fattuale (fondamentale, grafici 5 timeframe, option chain,
    comparative) per un ticker USA. Un riassunto numerico appare SUBITO nel
    pannello del canale; il documento completo con i grafici si apre
    AUTOMATICAMENTE in una finestra dedicata (con pulsante Salva) non
    appena finisci di scrivere la tua risposta, che viene fusa in coda al
    documento — non ripetere i dati fattuali. Ritorna un JSON {'summary',
    'report_markdown','channel_summary'}: 'summary' è il contenuto fattuale
    senza le immagini, per il TUO ragionamento; 'report_markdown' è il
    documento completo (con i grafici) su cui la tua risposta verrà fusa
    prima di apparire nella finestra; 'channel_summary' è già mostrato nel
    pannello, non lo scrivi tu."""
    full_markdown, channel_summary = report.build_report(_source, ticker)
    summary = report.strip_chart_images(full_markdown)
    return json.dumps({"summary": summary, "report_markdown": full_markdown, "channel_summary": channel_summary})


@mcp.tool()
def list_stocks(country: str = "USA") -> str:
    """Elenco tabellare dei titoli conosciuti (Nome, Asset, Paese, Indice di
    appartenenza), filtrato per paese. Solo 'USA' è supportato oggi — un
    paese diverso ritorna un messaggio leggibile, non una tabella vuota.
    La finestra con la tabella completa si apre SUBITO (a differenza di
    stock_report, qui non c'è alcuna narrativa da scrivere: non ripetere
    la tabella nella tua risposta, conferma solo quanti titoli sono stati
    trovati)."""
    if not stock_list.is_country_supported(country):
        return stock_list.unsupported_country_message(country)
    # Normalizzato in maiuscolo una sola volta: sia il titolo del documento
    # sia il channel_summary devono mostrare "USA", non "usa", anche se
    # l'AI/utente ha chiamato il tool con un casing diverso (build_rows fa
    # la stessa normalizzazione internamente per la colonna Paese).
    country = country.upper()
    rows = stock_list.build_rows(_tickers, country)
    table = stock_list.render_table(rows)
    channel_summary = f"{len(rows)} titoli (filtro: {country}). Elenco completo nella finestra."
    full_markdown = f"# Elenco titoli — {country}\n\n{table}"
    return json.dumps({"summary": channel_summary, "report_markdown": full_markdown, "channel_summary": channel_summary})


@mcp.tool()
def list_screeners() -> str:
    """Elenca gli screener disponibili in questo canale (id, titolo,
    descrizione). Usalo quando l'utente chiede di eseguire uno screener
    SENZA nominarne uno specifico, o quando il nome che usa non
    corrisponde con sicurezza a un id noto. Apre una finestra di
    selezione: non ripetere l'elenco in chat, conferma solo quanti
    screener sono disponibili."""
    items = screeners.list_items()
    return json.dumps({
        "summary": f"Ho aperto la finestra di selezione con {len(items)} screener disponibili.",
        "items": items,
    })


@mcp.tool()
def run_screener(screener_id: str, top: int = 25) -> str:
    """Esegue lo screener identificato da screener_id, con selezione e
    metriche proprie di quello screener (vedi list_screeners per gli id
    disponibili se non li conosci — non indovinare). La finestra col
    documento si apre a fine turno: la risposta include, in coda al
    riassunto, le istruzioni di giudizio finale SPECIFICHE di questo
    screener — la tua risposta successiva è ESCLUSIVAMENTE quanto
    richiesto da quelle istruzioni. Ritorna un JSON
    {'summary','report_markdown','channel_summary','title'
    [,'title_suffix']}, oppure {'error': '...'} se screener_id non è
    valido."""
    return json.dumps(screeners.dispatch(
        screener_id, _source, _tickers, FUNDAMENTALS_CACHE_PATH, DISCOVERIES_PATH, top=top))


@mcp.tool()
def test_market_data_source() -> str:
    """Verifica se la fonte dati mercato ATTIVA (da market_data.json) è
    raggiungibile. Usalo SOLO su richiesta esplicita di test/diagnostica —
    non fa parte del flusso normale di stock_report/screener. Ritorna JSON
    {'ok': bool, 'message': str}."""
    ok, message = _source.test_connection()
    return json.dumps({"ok": ok, "message": message})


if __name__ == "__main__":
    mcp.run(transport="stdio")
