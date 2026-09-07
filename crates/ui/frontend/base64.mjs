// Decodifica pura di un chunk `pty-out` (stringa base64 → byte grezzi).
// `atob` è globale sia nel webview (Chromium) sia in Node 16+: nessuna
// dipendenza da `Buffer` (che non esiste nel browser), stesso codice nei
// due ambienti — per questo la funzione si può testare con node:test.
export function base64ToUint8Array(b64) {
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}
