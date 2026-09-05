"""Appartenenza a indice per la colonna 'Indice' di list_stocks — tre insiemi
di ticker curati a mano, nessuna chiamata di rete (non esiste un endpoint
ufficiale gratuito equivalente al company_tickers.json di SEC per "chi è in
un indice oggi": è un prodotto commerciale di S&P/Nasdaq/Dow Jones). Stesso
compromesso già accettato per peers.py::SECTOR_PEERS: liste statiche,
invecchiano quando un indice cambia composizione, vanno aggiornate a mano.
Vedi Docs/superpowers/specs/2026-07-24-financial-markets-list-stocks-design.md §4.
"""

DOW_JONES_TICKERS: set[str] = {
    "AAPL", "AMGN", "AMZN", "AXP", "BA", "CAT", "CRM", "CSCO", "CVX", "DIS",
    "GS", "HD", "HON", "IBM", "JNJ", "JPM", "KO", "MCD", "MMM", "MRK",
    "MSFT", "NKE", "NVDA", "PG", "SHW", "TRV", "UNH", "V", "VZ", "WMT",
}

NASDAQ100_TICKERS: set[str] = {
    "AAPL", "MSFT", "NVDA", "AMZN", "META", "GOOGL", "GOOG", "AVGO", "TSLA", "COST",
    "NFLX", "AMD", "PEP", "ADBE", "LIN", "CSCO", "TMUS", "QCOM", "INTU", "AMAT",
    "TXN", "CMCSA", "AMGN", "HON", "BKNG", "ISRG", "VRTX", "PANW", "ADP", "GILD",
    "SBUX", "MU", "LRCX", "MDLZ", "ADI", "REGN", "KLAC", "PYPL", "SNPS", "CDNS",
    "MELI", "CSX", "MAR", "CTAS", "ORLY", "ASML", "PDD", "CRWD", "ABNB", "WDAY",
    "ROP", "NXPI", "MRVL", "FTNT", "MNST", "PCAR", "ROST", "DXCM", "PAYX", "ODFL",
    "AEP", "KHC", "EA", "FAST", "EXC", "CPRT", "VRSK", "XEL", "CTSH", "CCEP",
    "TTD", "IDXX", "DDOG", "ANSS", "BIIB", "ON", "ZS", "GEHC", "TEAM", "CDW",
    "FANG", "WBD", "MCHP", "GFS", "ILMN", "KDP", "DLTR", "ALGN", "ENPH", "SIRI",
    "LCID", "JD", "RIVN", "ZM",
}

SP500_TICKERS: set[str] = {
    # Information Technology
    "AAPL", "MSFT", "NVDA", "AVGO", "ORCL", "ADBE", "CRM", "CSCO", "ACN", "AMD",
    "INTC", "IBM", "QCOM", "TXN", "INTU", "NOW", "AMAT", "MU", "ADI", "LRCX",
    "KLAC", "SNPS", "CDNS", "PANW", "CRWD", "FTNT", "ANSS", "ROP", "GLW", "HPQ",
    "HPE", "DELL", "JNPR", "NTAP", "WDC", "STX", "TER", "KEYS", "TDY", "TRMB",
    "ZBRA", "SWKS", "QRVO", "MPWR", "ENPH", "FSLR", "GEN", "AKAM", "FFIV", "EPAM",
    "GDDY", "PTC", "CTSH", "IT", "MSI",
    # Communication Services
    "GOOGL", "GOOG", "META", "NFLX", "DIS", "CMCSA", "T", "VZ", "TMUS", "CHTR",
    "EA", "TTWO", "WBD", "OMC", "IPG", "NWSA", "NWS", "FOXA", "FOX", "LYV", "MTCH",
    # Consumer Discretionary
    "AMZN", "TSLA", "HD", "MCD", "NKE", "LOW", "SBUX", "TJX", "BKNG", "CMG",
    "ORLY", "MAR", "GM", "F", "HLT", "YUM", "ROST", "AZO", "DHI", "LEN",
    "NVR", "PHM", "EBAY", "ETSY", "DPZ", "BBY", "GRMN", "POOL", "ULTA", "RL",
    "TPR", "EXPE", "CCL", "RCL", "NCLH", "LVS", "WYNN", "MGM", "APTV", "LKQ",
    "KMX", "DRI", "WHR",
    # Consumer Staples
    "PG", "KO", "PEP", "COST", "WMT", "PM", "MO", "MDLZ", "CL", "KMB",
    "GIS", "KHC", "STZ", "SYY", "HSY", "MKC", "CHD", "CLX", "TSN", "HRL",
    "CAG", "CPB", "K", "ADM", "KR", "TGT", "DG", "DLTR", "EL", "KDP", "MNST",
    "KVUE",
    # Health Care
    "UNH", "JNJ", "LLY", "ABBV", "MRK", "PFE", "TMO", "ABT", "DHR", "BMY",
    "AMGN", "MDT", "ISRG", "GILD", "VRTX", "CVS", "ELV", "CI", "HUM", "ZTS",
    "BSX", "SYK", "REGN", "BDX", "HCA", "IDXX", "IQV", "A", "RMD", "MRNA",
    "BIIB", "MTD", "WAT", "WST", "DXCM", "ALGN", "ZBH", "BAX", "GEHC", "DVA",
    "CAH", "MCK", "COR", "LH", "DGX", "INCY", "VTRS", "HOLX", "TECH", "CRL",
    "MOH", "CNC", "UHS",
    # Financials
    "BRK-B", "JPM", "V", "MA", "BAC", "WFC", "GS", "MS", "SPGI", "AXP",
    "C", "SCHW", "BLK", "CB", "PGR", "MMC", "ICE", "CME", "AON", "USB",
    "PNC", "TFC", "AIG", "MET", "PRU", "TRV", "AFL", "ALL", "AJG", "MCO",
    "COF", "BK", "STT", "FITB", "HBAN", "RF", "KEY", "CFG", "NTRS", "SYF",
    "DFS", "TROW", "AMP", "IVZ", "BEN", "WTW", "GL", "L", "RJF", "MSCI",
    "FDS", "NDAQ", "CBOE", "MKTX", "PYPL", "FIS", "FI", "GPN", "JKHY", "WU",
    # Industrials
    "HON", "UNP", "RTX", "BA", "GE", "CAT", "DE", "LMT", "UPS", "ADP",
    "GD", "NOC", "MMM", "ETN", "ITW", "EMR", "PH", "CSX", "NSC", "WM",
    "RSG", "CTAS", "PCAR", "CMI", "FDX", "JCI", "IR", "ROK", "DOV", "XYL",
    "AME", "FAST", "PWR", "GWW", "EFX", "VRSK", "EXPD", "ODFL", "CHRW", "LDOS",
    "HII", "TXT", "TT", "CARR", "OTIS", "LHX", "HWM", "BR", "PAYX", "PAYC",
    "NDSN", "ALLE", "SNA", "SWK", "MAS",
    # Energy
    "XOM", "CVX", "COP", "SLB", "EOG", "MPC", "PSX", "VLO", "OXY", "WMB",
    "KMI", "OKE", "HES", "DVN", "HAL", "BKR", "FANG", "TRGP", "CTRA", "EQT",
    "MRO", "APA",
    # Materials
    "LIN", "SHW", "APD", "ECL", "FCX", "NEM", "DOW", "DD", "PPG", "NUE",
    "VMC", "MLM", "ALB", "CE", "IFF", "PKG", "AVY", "BALL", "IP", "MOS",
    "CF", "LYB", "STLD", "AMCR", "EMN",
    # Real Estate
    "PLD", "AMT", "EQIX", "PSA", "WELL", "SPG", "O", "DLR", "CCI", "VICI",
    "SBAC", "AVB", "EQR", "EXR", "MAA", "ESS", "INVH", "UDR", "CPT", "ARE",
    "KIM", "REG", "BXP", "HST", "VTR", "IRM",
    # Utilities
    "NEE", "DUK", "SO", "D", "AEP", "EXC", "SRE", "XEL", "ED", "WEC",
    "PEG", "ES", "FE", "EIX", "ETR", "AEE", "CMS", "CNP", "DTE", "PPL",
    "ATO", "NI", "LNT", "EVRG", "PNW",
}

_INDEX_SETS: list[tuple[str, set[str]]] = [
    ("S&P 500", SP500_TICKERS),
    ("Dow Jones", DOW_JONES_TICKERS),
    ("Nasdaq-100", NASDAQ100_TICKERS),
]


def indices_for(ticker: str) -> str:
    """'S&P 500, Dow Jones' (ordine fisso: S&P 500, Dow Jones, Nasdaq-100),
    oppure '-' se il ticker non è in nessuno dei tre. Case-insensitive:
    tickers_us.json usa sempre maiuscolo, ma questa funzione non lo assume."""
    normalized = ticker.upper()
    names = [name for name, members in _INDEX_SETS if normalized in members]
    return ", ".join(names) if names else "-"
