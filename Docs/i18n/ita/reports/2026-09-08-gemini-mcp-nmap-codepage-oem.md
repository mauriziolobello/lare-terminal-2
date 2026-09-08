## Report per il supervisore

### Compito assegnato
Fix codepage OEM in `mcp-nmap` (recupero da v1): risolvere il mojibake sulle etichette accentate italiane (`Sì` → `S`, U+FFFD) prodotte dai comandi diagnostici di rete nativi di Windows (`ipconfig`, `arp`, `route`, `netstat`, `tracert`) invocati da `local_network_info` e `traceroute` in `crates/mcp-nmap/src/network_info.rs`.

### Cosa ho fatto
1. **Analisi della causa radice**: `run_and_capture` decodificava i flussi stdout/stderr usando direttamente `String::from_utf8_lossy` sui byte grezzi. I processi nativi Win32 su console Windows scrivono invece nel codepage OEM (CP850 in Italia, CP437 su macchine in lingua inglese US). I byte accentati OEM non UTF-8 venivano quindi convertiti nel carattere U+FFFD (``).
2. **Dipendenze e Win32 features**:
   - Aggiunta la dipendenza `oem_cp = "2"` in `crates/mcp-nmap/Cargo.toml` con commento didattico.
   - Aggiunte le feature `"Win32_System_Console"` e `"Win32_Globalization"` a `windows = "0.62"` sotto `[target.'cfg(windows)'.dependencies]`.
3. **TDD (RED reale)**:
   - Aggiunta prima la funzione stub `decode_oem` ripiegata su `String::from_utf8_lossy`.
   - Scritti i test unitari `decode_oem_decodifica_correttamente_accentate_cp850`, `decode_oem_codepage_sconosciuta_ripiega_su_utf8_lossy` e `decode_oem_preserva_invariati_i_dati_ascii_puri` in `crates/mcp-nmap/src/network_info.rs`.
   - Eseguito `cargo test -p mcp-nmap decode_oem` e confermato il fallimento RED sull'asserzione di `[0x53, 0x8D]` ("Sì" in CP850): `left: "S\u{FFFD}", right: "Sì"`.
4. **Implementazione (GREEN reale)**:
   - Implementata la funzione pura `decode_oem(bytes: &[u8], codepage: u32) -> String`: converte in modo verificato `u16::try_from(codepage)`, cerca nella tabella `DECODING_TABLE_CP_MAP` di `oem_cp` e chiama `table.decode_string_lossy(bytes)`. Se il codepage non è presente o eccede `u16::MAX`, applica il fallback contrattuale non-negoziabile su `String::from_utf8_lossy(bytes)`.
   - Implementata `active_console_output_codepage() -> u32` (`#[cfg(windows)]`): chiama `GetConsoleOutputCP()`. **Punto critico gestito**: quando `mcp-nmap.exe` gira senza console (lanciato dall'orchestratore con `CREATE_NO_WINDOW`), `GetConsoleOutputCP()` restituisce `0`; in tal caso ripiega su `GetOEMCP()`, che ritorna sempre il codepage OEM di default di sistema.
   - Aggiornata `run_and_capture` per decodificare stdout e stderr usando `active_console_output_codepage()` e `decode_oem` su Windows, mantenendo il fallback UTF-8 lossy su piattaforme non-Windows.
5. **Verifica end-to-end e manual smoke check**:
   - Eseguito test con piping JSON-RPC simulando l'avvio con `CREATE_NO_WINDOW = $true` (processo senza finestra di console, come avviato dall'orchestratore). L'output di `local_network_info` ha confermato la corretta lettura di `"DHCP abilitato : Sì"` tramite il fallback su `GetOEMCP()`.

### File toccati
Elenco esplicito dei file modificati e creati:
- `crates/mcp-nmap/Cargo.toml` (modificato — bump versione a 2.0.1, aggiunta dipendenza `oem_cp = "2"` con commento didattico, feature Windows `Win32_System_Console` e `Win32_Globalization`)
- `Cargo.lock` (modificato — risoluzione delle dipendenze `oem_cp 2.1.2`, `phf 0.11.3`, `phf_shared 0.11.3`)
- `crates/mcp-nmap/src/network_info.rs` (modificato — implementazione `decode_oem`, `active_console_output_codepage`, aggiornamento `run_and_capture`, aggiunti 3 test unitari)
- `crates/mcp-nmap/CHANGELOG.md` (modificato — aggiunta voce `[2.0.1]` in cima con sintesi del bug, fix, dipendenze, TDD e verifiche)
- `crates/mcp-nmap/IMPLEMENTATION.md` (modificato — aggiornato header versione `2.0.1` e documentata la soluzione mojibake OEM in `network_info.rs` con firme aggiornate)
- `Docs/i18n/ita/KNOWN-ISSUES.md` (modificato — stato aggiornato da `[APERTO]` a `[PARZIALE]`, specificando la risoluzione in `mcp-nmap` 2.0.1 e delimitando la parte ancora aperta in `mcp-server/src/session.rs`)
- `Docs/i18n/ita/HANDOFF.md` (modificato — aggiornata riga `mcp-nmap 2.0.1` nelle versioni correnti e aggiunta voce descrittiva nella sezione `FATTO`)
- `Docs/i18n/ita/reports/2026-09-08-gemini-mcp-nmap-codepage-oem.md` (creato — questo documento di report)

Riscontro `git diff --stat main..HEAD` prima dell'aggiunta del report:
```text
 Cargo.lock                          |  50 ++++++++++++----
 Docs/i18n/ita/HANDOFF.md            |  12 +++-
 Docs/i18n/ita/KNOWN-ISSUES.md       |  23 ++++---
 crates/mcp-nmap/CHANGELOG.md        |  34 +++++++++++
 crates/mcp-nmap/Cargo.toml          |  15 ++++-
 crates/mcp-nmap/IMPLEMENTATION.md   |  35 ++++++++---
 crates/mcp-nmap/src/network_info.rs | 114 +++++++++++++++++++++++++++++++-----
 7 files changed, 237 insertions(+), 46 deletions(-)
```

### Branch e commit
- Branch dedicato: `fix/mcp-nmap-codepage-oem`
- `git log --oneline main..HEAD`:
```text
2aa2d2c fix(mcp-nmap): codepage OEM per comandi nativi Win32 in network_info.rs (2.0.1)
```

### Esito reale dei comandi di verifica

1. **`cargo test -p mcp-nmap`**:
```text
running 64 tests
test elevate::tests::timeout_display_is_readable_and_distinct_from_confirm_gate ... ok
test elevate::tests::build_parameters_rejects_an_argument_containing_a_quote_character ... ok
test markdown::tests::invocation_label_local_network_info_has_no_target ... ok
test elevate::tests::scan_timeout_constant_exceeds_the_180s_confirm_gate ... ok
test elevate::tests::unknown_error_code_maps_to_other_with_code_in_message ... ok
test markdown::tests::invocation_label_host_discovery_shows_target ... ok
test markdown::tests::invocation_label_os_detect_mentions_elevation ... ok
test elevate::tests::error_cancelled_maps_to_user_cancelled_variant ... ok
test markdown::tests::invocation_label_traceroute_shows_target ... ok
test markdown::tests::invocation_label_unknown_tool_falls_back ... ok
test elevate::tests::build_parameters_quotes_an_argument_containing_whitespace ... ok
test markdown::tests::invocation_label_version_scan_shows_target ... ok
test markdown::tests::markdown_report_down_host_has_no_ports_section ... ok
test elevate::tests::build_parameters_leaves_whitespace_free_arguments_unquoted ... ok
test markdown::tests::invocation_label_quick_scan ... ok
test markdown::tests::invocation_label_vuln_scan_names_the_script_category ... ok
test markdown::tests::markdown_report_empty_hosts_says_so ... ok
test markdown::tests::markdown_report_includes_host_level_script_output ... ok
test markdown::tests::markdown_report_includes_os_matches_when_os_detection_true ... ok
test markdown::tests::markdown_report_includes_per_port_script_output ... ok
test markdown::tests::markdown_report_includes_target_host_and_ports ... ok
test markdown::tests::markdown_report_without_scripts_has_no_script_sections ... ok
test markdown::tests::summary_counts_up_hosts_and_open_ports ... ok
test markdown::tests::summary_with_scripts_reports_script_count_not_a_vulnerable_verdict ... ok
test markdown::tests::summary_without_scripts_matches_existing_wording_exactly ... ok
test network_info::tests::decode_oem_codepage_sconosciuta_ripiega_su_utf8_lossy ... ok
test markdown::tests::summary_zero_hosts_says_none_active_not_an_error ... ok
test network_info::tests::decode_oem_decodifica_correttamente_accentate_cp850 ... ok
test network_info::tests::decode_oem_preserva_invariati_i_dati_ascii_puri ... ok
test network_info::tests::network_info_outcome_json_roundtrips ... ok
test network_info::tests::traceroute_rejects_invalid_target_before_spawning_anything ... ok
test network_info::tests::traceroute_rejects_target_with_whitespace ... ok
test report::tests::malformed_xml_is_a_readable_parse_error_not_a_panic ... ok
test report::tests::empty_hosts_list_when_no_host_elements_present ... ok
test report::tests::parses_host_down_as_legitimate_zero_result_not_error ... ok
test report::tests::parses_host_up_with_open_and_closed_ports ... ok
test scan::tests::host_discovery_nmap_not_available_is_a_readable_error_before_any_process_call ... ok
test report::tests::parses_host_level_script_results_using_output_attribute_not_text_child ... ok
test report::tests::parses_os_detect_with_osmatches_and_filtered_port ... ok
test scan::tests::os_detect_invalid_target_is_a_readable_error ... ok
test report::tests::parses_per_port_script_results ... ok
test scan::tests::os_detect_nmap_not_available_is_a_readable_error_before_elevation ... ok
test scan::tests::quick_scan_invalid_target_is_a_readable_error_before_any_process_call ... ok
test report::tests::parses_real_nmap_output_including_doctype_and_stylesheet_pi ... ok
test scan::tests::quick_scan_nmap_not_available_is_a_readable_error_before_any_process_call ... ok
test scan::tests::validate_target_accepts_a_cidr_range ... ok
test scan::tests::validate_target_accepts_a_hyphenated_hostname ... ok
test scan::tests::validate_target_accepts_a_normal_ip ... ok
test scan::tests::validate_target_accepts_an_ipv4_range ... ok
test scan::tests::validate_target_accepts_an_octet_wildcard ... ok
test scan::tests::validate_target_rejects_a_target_starting_with_dash ... ok
test scan::tests::validate_target_rejects_a_target_containing_whitespace ... ok
test scan::tests::version_scan_invalid_target_is_a_readable_error_before_any_process_call ... ok
test scan::tests::validate_target_rejects_disallowed_characters ... ok
test scan::tests::validate_target_rejects_empty_string ... ok
test scan::tests::vuln_scan_invalid_target_is_a_readable_error_before_any_process_call ... ok
test scan::tests::host_discovery_uses_sn_flag_and_produces_summary ... ok
test scan::tests::quick_scan_malformed_xml_is_a_readable_error_not_a_panic ... ok
test scan::tests::quick_scan_nonzero_exit_is_a_readable_error ... ok
test scan::tests::quick_scan_success_produces_summary_and_report ... ok
test scan::tests::version_scan_uses_sv_flag_and_produces_summary ... ok
test scan::tests::vuln_scan_always_bounds_script_execution_time ... ok
test scan::tests::vuln_scan_always_uses_the_fixed_vuln_category_never_a_custom_script ... ok
test scan::tests::vuln_scan_uses_script_vuln_and_produces_summary ... ok

test result: ok. 64 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s
```

2. **`cargo clippy -p mcp-nmap --all-targets`**:
```text
    Checking mcp-nmap v2.0.1 (C:\Users\Maurizio\Documents\Progetti\Lare Terminal 2.0\crates\mcp-nmap)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.94s
```
(Zero warning).

3. **`cargo build -p mcp-nmap`**:
```text
   Compiling mcp-nmap v2.0.1 (C:\Users\Maurizio\Documents\Progetti\Lare Terminal 2.0\crates\mcp-nmap)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.09s
```

4. **`cargo fmt -p mcp-nmap --check`**:
Il file toccato `crates/mcp-nmap/src/network_info.rs` è pulito al 100%. L'unico drift mostrato è quello preesistente in `markdown.rs` (5 diff), documentato nel changelog fin dalla versione 0.3.0.

5. **`cargo test` (intero workspace, default-members)**:
Tutti i test del workspace completati con esito positivo:
- `mcp-nmap`: 64 passed, 0 failed
- `mcp-server`: 120 passed, 0 failed
- `ping`: 2 passed, 0 failed
- `plugin-protocol`: 9 passed, 0 failed
- `protocol`: 80 passed, 0 failed
- `startup-config`: 22 passed, 0 failed
- `orchestrator`: 1187 passed, 0 failed

6. **Verifica dal vivo con `CREATE_NO_WINDOW = $true` (processo senza console)**:
Comando eseguito via pwsh:
```powershell
$psi = New-Object System.Diagnostics.ProcessStartInfo;
$psi.FileName = ".\target\debug\mcp-nmap.exe";
$psi.CreateNoWindow = $true;
$psi.UseShellExecute = $false;
$psi.RedirectStandardInput = $true;
$psi.RedirectStandardOutput = $true;
$p = [System.Diagnostics.Process]::Start($psi);
$p.StandardInput.WriteLine('{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}');
$p.StandardInput.WriteLine('{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"local_network_info","arguments":{}}}');
$p.StandardInput.Close();
$stdout = $p.StandardOutput.ReadToEnd();
$p.WaitForExit();
$stdout.Substring($stdout.IndexOf("DHCP abilitato"), 60)
```
Output effettivo ottenuto:
```text
DHCP abilitato. . . . . . . . . . . . : Sì\r\n   Configura
```
Dimostra che anche in assenza di console agganciata (quando `GetConsoleOutputCP()` restituisce `0`), il fallback su `GetOEMCP()` interroga con successo il codepage di sistema (CP850) e decodifica `Sì` senza alcun mojibake.

### Deviazioni dal compito assegnato
1. In `crates/mcp-nmap/src/main.rs`, alla riga 200, è presente la stringa `tracing::info!("Lare Terminal mcp-nmap v0.8.2 starting (transport: stdio)");` (ereditata dalla v1 e non aggiornata a `2.0.0` nel fork iniziale). In conformità alle istruzioni del briefing che imponevano categoricamente di toccare un solo file di codice (`crates/mcp-nmap/src/network_info.rs`) evitando estensioni arbitrarie di scope, `main.rs` è stato lasciato invariato. Si suggerisce di allineare la stringa di log in una successiva revisione.
2. In `Docs/i18n/ita/KNOWN-ISSUES.md`, il tag della voce "Codepage — output dei comandi NATIVI" è stato aggiornato da `[APERTO]` a `[PARZIALE]` per distinguere che la problematica è risolta in `mcp-nmap`, mentre rimane aperta per `crates/mcp-server/src/session.rs`.

### Documentazione aggiornata
- `crates/mcp-nmap/Cargo.toml`: versione incrementata a `2.0.1`, aggiunta dipendenza `oem_cp` e features Windows.
- `crates/mcp-nmap/CHANGELOG.md`: aggiunta voce dettagliata `[2.0.1]`.
- `crates/mcp-nmap/IMPLEMENTATION.md`: aggiornata versione in testata e documentata l'architettura della decodifica OEM in `network_info.rs`.
- `Docs/i18n/ita/KNOWN-ISSUES.md`: aggiornata la sezione codepage allo stato `[PARZIALE]`.
- `Docs/i18n/ita/HANDOFF.md`: aggiornata la tabella "Versioni correnti" e inserita la voce in `FATTO`.
Tutti gli aggiornamenti di documentazione sono stati tracciati e committati insieme al codice.

### Cosa NON ho potuto verificare
- Non è stata aperta manualmente l'interfaccia grafica interattiva di `ui.exe` per digitare a mano nel canale `/nmap` (ambiente agentic headless). Tuttavia, come dimostrato al punto 6 delle verifiche, è stata testata dal vivo l'esecuzione del binario `mcp-nmap.exe` compilato in modalità `CREATE_NO_WINDOW`, che replica fedelmente l'esatto contesto operativo con cui l'orchestratore invoca il sidecar.
