// Stato puro della barra segnalini della finestra terminale (piano 3):
// ultimo comando intercettato (OSC 9001) e "AI al lavoro" (ActivityIndicator,
// Task 5). Nessun DOM qui — terminal.js legge questo stato e lo riflette
// nell'HTML; qui solo le transizioni, testabili da sole.
export function createIndicatorState() {
  return { lastCommand: null, aiBusy: false };
}

export function onIntercept(state, line) {
  return { ...state, lastCommand: line };
}

export function onActivity(state, sessionId, ownSessionId, on) {
  // Un ActivityIndicator per una sessione diversa dalla nostra finestra va
  // ignorato: l'MVP ha una sola finestra terminale per processo, ma il
  // filtro costa nulla e previene un bug quando ne arriverà una seconda.
  if (sessionId !== ownSessionId) return state;
  return { ...state, aiBusy: on };
}
