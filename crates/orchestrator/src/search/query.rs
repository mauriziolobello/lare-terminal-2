//! Parsing della query di ricerca → Matcher sul NOME del file.
//!
//! Tre modalità, valutate in ordine:
//! 1. `re:<pattern>` ⇒ regex (case-insensitive) compilata con `regex::RegexBuilder`.
//! 2. `*`/`?` ⇒ glob (case-insensitive sul solo nome file).
//! 3. altrimenti ⇒ token-AND (tutte le parole devono comparire, case-insensitive, ordine libero).
use globset::Glob;

/// Chiave e valore riconosciuto per la direttiva `folder:` — vedi
/// `extract_folder_directive`. `from-here` è il PRIMO di un numero
/// potenzialmente crescente di valori futuri (spec §1): la funzione è
/// strutturata con un confronto esplicito sul valore proprio per poter
/// accogliere un secondo valore riconosciuto in futuro senza riscrittura.
const FOLDER_PREFIX: &str = "folder:";
const FOLDER_FROM_HERE: &str = "from-here";

/// Risultato del parsing dell'input grezzo di `/find`: le tre direttive
/// indipendenti e componibili (`in:"<frase>"`, `folder:from-here`, query sul
/// nome), in qualunque ordine reciproco nell'input originale.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedFind {
    /// Frase di ricerca contenuto, se `in:"<frase>"` era presente.
    pub content: Option<String>,
    /// `true` se `folder:from-here` era presente — la ricerca deve usare
    /// SOLO il root Cwd (vedi `roots::resolve_roots`, parametro `only_cwd`).
    pub from_here: bool,
    /// Query sul nome file, pronta per `parse_query`.
    pub name_query: String,
}

/// Estrae `folder:<parola>` da `input`, ovunque compaia — nessuna virgoletta,
/// il valore è delimitato da whitespace. Ritorna:
/// - `Ok(Some(resto))` se trovato con valore `from-here` (`resto` = input con
///   quel token rimosso, trimmato);
/// - `Err(...)` se trovato con un valore diverso da `from-here` — un `folder:`
///   con la chiave riconosciuta ma il valore sbagliato è un errore utente
///   esplicito, non un degrado silenzioso (spec §1);
/// - `Ok(None)` se `folder:` (minuscolo esatto) non compare affatto — incluso
///   il caso in cui compaia con un'altra capitalizzazione (`Folder:`), che
///   semplicemente non fa match sulla ricerca case-sensitive del prefisso e
///   quindi resta testo letterale nella query sul nome (comportamento del
///   chiamante, non di questa funzione).
fn extract_folder_directive(input: &str) -> Result<Option<String>, String> {
    if let Some(start) = input.find(FOLDER_PREFIX) {
        let after_prefix = start + FOLDER_PREFIX.len();
        let value_end = input[after_prefix..]
            .find(char::is_whitespace)
            .map(|rel| after_prefix + rel)
            .unwrap_or(input.len());
        let value = &input[after_prefix..value_end];
        if value != FOLDER_FROM_HERE {
            return Err(format!(
                "folder: valore non riconosciuto {value:?} (atteso \"{FOLDER_FROM_HERE}\")"
            ));
        }
        let mut rest = String::new();
        rest.push_str(input[..start].trim());
        rest.push(' ');
        rest.push_str(input[value_end..].trim());
        return Ok(Some(rest.trim().to_string()));
    }
    Ok(None)
}

/// Estrae `in:"<frase>"` da `input` — logica INVARIATA rispetto a prima
/// dell'introduzione di `folder:` (ex corpo di `parse_find_input`), tranne
/// che non applica più da sola il fallback `"*"` — quello ora vive in
/// `parse_find_input`, generalizzato anche a `folder:from-here` (vedi sotto).
/// Non può fallire: una virgoletta di apertura senza chiusura è "nessuna
/// direttiva", non un errore.
fn parse_in_directive(input: &str) -> (Option<String>, String) {
    const PREFIX: &str = "in:\"";
    if let Some(start) = input.find(PREFIX) {
        let after_prefix = start + PREFIX.len();
        if let Some(rel_end) = input[after_prefix..].find('"') {
            let phrase = input[after_prefix..after_prefix + rel_end].to_string();
            let mut rest = String::new();
            rest.push_str(input[..start].trim());
            rest.push(' ');
            rest.push_str(input[after_prefix + rel_end + 1..].trim());
            return (Some(phrase), rest.trim().to_string());
        }
    }
    (None, input.to_string())
}

/// Estrae le direttive `folder:<parola>` e `in:"<frase>"` dall'input grezzo di
/// `/find`, ovunque compaiano, in qualunque ordine reciproco NELL'INPUT
/// dell'utente. Vedi `extract_folder_directive`/`parse_in_directive` per i
/// dettagli di ciascuna.
///
/// ORDINE DI ESTRAZIONE (non l'ordine in cui compaiono nell'input, che è
/// libero): `in:"<frase>"` viene estratta PRIMA, `folder:` DOPO, sul resto.
/// Necessario perché `extract_folder_directive` cerca `folder:` come
/// sottostringa grezza ovunque nell'input — se venisse eseguita per prima,
/// un valore letterale `folder:` dentro una frase `in:"..."` (es. una ricerca
/// di contenuto su un file JSON/log) verrebbe scambiato per la direttiva,
/// rompendo o corrompendo la frase di ricerca contenuto. Estraendo prima
/// `in:"..."`, quella porzione di testo viene rimossa dall'input prima che
/// `extract_folder_directive` la veda.
///
/// `Err` SOLO se `folder:` è presente (nel testo rimasto dopo l'estrazione di
/// `in:"..."`) con un valore non riconosciuto — in tal caso l'intera funzione
/// ritorna subito (comportamento deterministico: un `folder:` invalido blocca
/// tutto, non produce un ibrido parzialmente parsato — la frase `in:` già
/// estratta viene scartata insieme al resto, non ritorna in un `ParsedFind`).
pub fn parse_find_input(input: &str) -> Result<ParsedFind, String> {
    // ORDINE: in: PRIMA di folder:, non come da spec letterale — vedi commit
    // che ha introdotto questa riga. Se folder: viene estratto per primo dal
    // testo grezzo, un valore letterale "folder:" DENTRO una frase in:"..."
    // viene scambiato per la direttiva (falso positivo): la ricerca sul
    // contenuto si rompe o corrompe la frase. Estraendo prima in:"...", la
    // frase tra virgolette non viene mai scansionata da extract_folder_directive.
    let (content, after_in) = parse_in_directive(input);

    let (from_here, name_query) = match extract_folder_directive(&after_in)? {
        Some(rest) => (true, rest),
        None => (false, after_in),
    };

    // Fallback "qualunque nome": se ALMENO UNA direttiva (`in:` o
    // `folder:from-here`) è stata riconosciuta e non resta query sul nome,
    // `""` andrebbe a `Matcher::Tokens(vec![])` che non matcha MAI nulla
    // (`empty_query_matches_nothing`) — sbagliato quando l'utente ha comunque
    // espresso un filtro: vuole "tutto ciò che c'è", non zero risultati
    // sempre. Un `/find` bare (nessuna direttiva, nessun nome) resta gestito
    // a monte da `ws.rs` (`query.is_empty()` sul testo grezzo, invariato).
    let name_query = if name_query.is_empty() && (content.is_some() || from_here) {
        "*".to_string()
    } else {
        name_query
    };

    Ok(ParsedFind { content, from_here, name_query })
}

pub enum Matcher {
    Glob(globset::GlobMatcher),
    Tokens(Vec<String>), // già in lowercase; vuoto ⇒ nessun match
    Regex(regex::Regex), // case-insensitive; già compilata
}

pub fn parse_query(q: &str) -> Matcher {
    let q = q.trim();

    // Modalità 1: regex — DEVE precedere il check glob (`re:foo*bar` è regex, non glob).
    if let Some(pat) = q.strip_prefix("re:") {
        return match regex::RegexBuilder::new(pat)
            .case_insensitive(true)
            .build()
        {
            Ok(re) => Matcher::Regex(re),
            Err(_) => Matcher::Tokens(Vec::new()), // regex invalida → nessun match
        };
    }

    // Modalità 2: glob — se la query contiene wildcards.
    if q.contains('*') || q.contains('?') {
        // Glob case-insensitive sul solo nome file.
        if let Ok(g) = Glob::new(&q.to_lowercase()) {
            return Matcher::Glob(g.compile_matcher());
        }
    }

    // Modalità 3: token-AND (default).
    Matcher::Tokens(
        q.split_whitespace().map(|t| t.to_lowercase()).collect(),
    )
}

impl Matcher {
    pub fn is_match(&self, filename: &str) -> bool {
        match self {
            Matcher::Glob(m) => m.is_match(filename.to_lowercase()),
            Matcher::Tokens(toks) => {
                if toks.is_empty() { return false; }
                let lower = filename.to_lowercase();
                toks.iter().all(|t| lower.contains(t))
            }
            // La case-insensitivity è già nel flag del regex compilato.
            Matcher::Regex(re) => re.is_match(filename),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn glob_when_star() {
        let m = parse_query("*.pdf");
        assert!(m.is_match("report.pdf"));
        assert!(!m.is_match("report.txt"));
    }
    #[test]
    fn glob_question_mark() {
        let m = parse_query("report-202?.xlsx");
        assert!(m.is_match("report-2024.xlsx"));
        assert!(!m.is_match("report-20244.xlsx"));
    }
    #[test]
    fn tokens_and_case_insensitive_order_free() {
        let m = parse_query("machine learning");
        assert!(m.is_match("Intro to Machine and Deep Learning.pdf"));
        assert!(m.is_match("learning_MACHINE.txt"));
        assert!(!m.is_match("machine.txt")); // manca "learning"
    }
    #[test]
    fn empty_query_matches_nothing() {
        assert!(!parse_query("").is_match("x.pdf"));
    }

    #[test]
    fn regex_anchored() {
        let m = parse_query("re:^report-\\d{4}\\.pdf$");
        assert!(m.is_match("report-2024.pdf"));
        assert!(!m.is_match("report-204.pdf"));
        assert!(!m.is_match("xreport-2024.pdf"));
    }
    #[test]
    fn regex_unanchored_case_insensitive() {
        let m = parse_query("re:language");
        assert!(m.is_match("MyLanguage.pdf"));
        assert!(m.is_match("language.txt"));
        assert!(!m.is_match("lang.txt"));
    }
    #[test]
    fn regex_alternation() {
        let m = parse_query("re:(foo|bar)");
        assert!(m.is_match("FOObar.x"));
        assert!(m.is_match("xBARy"));
        assert!(!m.is_match("baz"));
    }
    #[test]
    fn regex_invalid_matches_nothing() {
        let m = parse_query("re:[unclosed");
        assert!(!m.is_match("anything"));
    }

    // ── parse_find_input ─────────────────────────────────────────────────────

    #[test]
    fn extracts_content_phrase_before_name_query() {
        let parsed = parse_find_input(r#"in:"pacchetti dati" *.pdf"#).unwrap();
        assert_eq!(parsed.content, Some("pacchetti dati".to_string()));
        assert_eq!(parsed.name_query, "*.pdf");
        assert!(!parsed.from_here);
    }

    #[test]
    fn extracts_content_phrase_after_name_query() {
        let parsed = parse_find_input(r#"*.pdf in:"totale""#).unwrap();
        assert_eq!(parsed.content, Some("totale".to_string()));
        assert_eq!(parsed.name_query, "*.pdf");
        assert!(!parsed.from_here);
    }

    #[test]
    fn content_only_defaults_name_query_to_match_all_glob() {
        // ATTENZIONE: `parse_query("")` esistente tratta la stringa vuota come
        // `Matcher::Tokens(vec![])`, che NON matcha nulla (vedi
        // `empty_query_matches_nothing` più sotto in questo stesso file) — NON
        // "qualunque nome". Senza questo fallback, `/find in:"TODO"` (nessuna
        // query sul nome) troverebbe zero file, contraddicendo la spec §2
        // ("Assente ⇒ qualunque nome"). `parse_find_input` compensa restituendo
        // `"*"` (glob universale) quando il resto è vuoto E almeno una direttiva
        // (`in:` o `folder:from-here`) è stata riconosciuta.
        let parsed = parse_find_input(r#"in:"TODO""#).unwrap();
        assert_eq!(parsed.content, Some("TODO".to_string()));
        assert_eq!(parsed.name_query, "*");
        assert!(!parsed.from_here);
    }

    #[test]
    fn no_in_directive_leaves_input_unchanged() {
        let parsed = parse_find_input("*.pdf").unwrap();
        assert_eq!(parsed.content, None);
        assert_eq!(parsed.name_query, "*.pdf");
        assert!(!parsed.from_here);
    }

    #[test]
    fn unclosed_quote_is_not_recognized_as_directive() {
        let parsed = parse_find_input(r#"in:"TODO"#).unwrap();
        assert_eq!(parsed.content, None, "nessuna virgoletta di chiusura ⇒ nessuna direttiva");
        assert_eq!(parsed.name_query, r#"in:"TODO"#, "input passato INVARIATO al matcher nome");
        assert!(!parsed.from_here);
    }

    #[test]
    fn empty_input_has_no_directive() {
        let parsed = parse_find_input("").unwrap();
        assert_eq!(parsed.content, None);
        assert_eq!(parsed.name_query, "");
        assert!(!parsed.from_here);
    }

    // ── folder:from-here ─────────────────────────────────────────────────────

    #[test]
    fn folder_from_here_combined_with_in_and_name_in_any_order() {
        let parsed = parse_find_input(r#"folder:from-here in:"totale" *.pdf"#).unwrap();
        assert!(parsed.from_here);
        assert_eq!(parsed.content, Some("totale".to_string()));
        assert_eq!(parsed.name_query, "*.pdf");
    }

    #[test]
    fn folder_from_here_after_name_query() {
        let parsed = parse_find_input("*.pdf folder:from-here").unwrap();
        assert!(parsed.from_here);
        assert_eq!(parsed.content, None);
        assert_eq!(parsed.name_query, "*.pdf");
    }

    #[test]
    fn folder_from_here_between_name_and_in() {
        let parsed = parse_find_input(r#"*.pdf folder:from-here in:"totale""#).unwrap();
        assert!(parsed.from_here);
        assert_eq!(parsed.content, Some("totale".to_string()));
        assert_eq!(parsed.name_query, "*.pdf");
    }

    #[test]
    fn folder_from_here_alone_falls_back_to_match_all_glob() {
        // Confermato dall'utente in brainstorming: folder:from-here da solo
        // (nessun nome, nessun in:) deve trovare "tutto nella cwd", non zero
        // risultati sempre — stessa generalizzazione del fallback "*" già
        // usata per in: da solo.
        let parsed = parse_find_input("folder:from-here").unwrap();
        assert!(parsed.from_here);
        assert_eq!(parsed.content, None);
        assert_eq!(parsed.name_query, "*");
    }

    #[test]
    fn folder_unknown_value_is_an_explicit_error() {
        let err = parse_find_input("folder:altrove").unwrap_err();
        assert!(err.contains("altrove"), "il messaggio deve nominare il valore rifiutato: {err}");
    }

    #[test]
    fn folder_key_wrong_case_is_not_recognized() {
        // La CHIAVE "folder:" è case-sensitive (minuscolo esatto) — "Folder:"
        // non è riconosciuta come direttiva: nessun errore, l'intero token
        // resta testo letterale nella query sul nome (stesso trattamento di
        // un errore di battitura, non un valore sbagliato su una chiave
        // riconosciuta).
        let parsed = parse_find_input("Folder:from-here").unwrap();
        assert!(!parsed.from_here);
        assert_eq!(parsed.name_query, "Folder:from-here");
    }

    #[test]
    fn folder_value_wrong_case_is_an_explicit_error() {
        // Qui la CHIAVE è riconosciuta (minuscolo esatto) ma il VALORE no —
        // "From-Here" != "from-here" — è un errore esplicito, non un degrado
        // silenzioso (a differenza del caso sulla chiave sopra).
        let err = parse_find_input("folder:From-Here").unwrap_err();
        assert!(err.contains("From-Here"));
    }

    #[test]
    fn in_phrase_containing_folder_colon_is_content_not_directive() {
        // Regressione trovata nella final review del branch: prima del fix,
        // "folder:" veniva estratto dal testo GREZZO prima di in:"...", quindi
        // una frase di ricerca contenuto che include letteralmente "folder:"
        // veniva scambiata per la direttiva folder: (falso positivo).
        let parsed = parse_find_input(r#"in:"vedi folder: qui""#).unwrap();
        assert_eq!(parsed.content, Some("vedi folder: qui".to_string()));
        assert!(!parsed.from_here);
    }

    #[test]
    fn in_phrase_with_from_here_substring_is_not_corrupted() {
        let parsed = parse_find_input(r#"in:"see the folder:from-here now""#).unwrap();
        assert_eq!(parsed.content, Some("see the folder:from-here now".to_string()));
        assert!(!parsed.from_here);
    }
}
