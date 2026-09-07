// Parsing puro del payload OSC 9001 emesso da `Osc.cs` (host lare-shell,
// spec §4.7): `lare;intercept;<riga intercettata>`. Estratto dall'handler
// inline di `registerOscHandler` (spike 2) per essere testabile senza
// xterm.js — terminal.js (Task 4) lo userà dentro quell'handler.
export function parseLareOsc(data) {
  const parts = data.split(";");
  if (parts[0] !== "lare" || parts[1] !== "intercept") return null;
  return { kind: "intercept", line: parts.slice(2).join(";") };
}
