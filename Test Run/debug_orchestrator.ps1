# Avvia l'orchestratore con i log anche su questa console (--console-log), oltre che nel file
# giornaliero in .\Configuration\logs\. Non serve per l'uso normale: sia ui.exe (modalità A) sia
# lare-shell.exe (modalità B) avviano l'orchestratore da soli se manca (self-heal, spec §6.4), ma
# lo fanno staccato e silenzioso, senza --console-log — usa questo script solo quando vuoi vedere
# i log dell'orchestratore live in una console, per debug. Nessuna variabile d'ambiente: la
# configurazione è in .\Configuration (default accanto all'eseguibile).
& "$PSScriptRoot\orchestrator.exe" --console-log
