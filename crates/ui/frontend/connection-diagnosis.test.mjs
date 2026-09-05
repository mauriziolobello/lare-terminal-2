// connection-diagnosis.test.mjs — test per buildDiagnosisMarkdown (node:test)
import { strict as assert } from "node:assert";
import { describe, it }     from "node:test";
import { buildDiagnosisMarkdown } from "./connection-diagnosis.js";

describe("buildDiagnosisMarkdown", () => {
  it("contiene ❌ per LARE_TOKEN quando non impostata", () => {
    const md = buildDiagnosisMarkdown(false, true);
    assert.match(md, /❌.*LARE_TOKEN|LARE_TOKEN.*❌/s);
  });

  it("contiene ✅ per LARE_TOKEN quando impostata", () => {
    const md = buildDiagnosisMarkdown(true, true);
    assert.match(md, /✅.*LARE_TOKEN|LARE_TOKEN.*✅/s);
  });

  it("contiene ❌ per la porta quando non raggiungibile", () => {
    const md = buildDiagnosisMarkdown(true, false);
    assert.match(md, /❌.*7331|7331.*❌/s);
  });

  it("contiene ✅ per la porta quando raggiungibile", () => {
    const md = buildDiagnosisMarkdown(true, true);
    assert.match(md, /✅.*7331|7331.*✅/s);
  });

  it("entrambi KO — mostra istruzioni per token E orchestrator", () => {
    const md = buildDiagnosisMarkdown(false, false);
    assert.ok(md.includes("LARE_TOKEN"), "deve citare LARE_TOKEN");
    assert.ok(md.includes("cargo run -p orchestrator"), "deve citare il comando orchestrator");
  });

  it("solo token mancante — mostra istruzione token, non quella orchestrator", () => {
    const md = buildDiagnosisMarkdown(false, true);
    assert.ok(md.includes("LARE_TOKEN"), "deve citare LARE_TOKEN");
    assert.ok(!md.includes("cargo run -p orchestrator"), "non deve citare orchestrator se la porta è aperta");
  });

  it("solo porta chiusa — mostra istruzione orchestrator, non token", () => {
    const md = buildDiagnosisMarkdown(true, false);
    assert.ok(md.includes("cargo run -p orchestrator"), "deve citare il comando orchestrator");
    assert.ok(!md.includes("non impostata"), "non deve dire che il token manca");
  });

  it("entrambi OK — suggerisce mismatch token", () => {
    const md = buildDiagnosisMarkdown(true, true);
    assert.ok(md.includes("token"), "deve menzionare il token come causa probabile");
  });

  it("include sempre la nota di retry automatico", () => {
    for (const [t, p] of [[true, true], [true, false], [false, true], [false, false]]) {
      const md = buildDiagnosisMarkdown(t, p);
      assert.ok(md.includes("automaticamente"), `caso [${t},${p}]: deve citare il retry automatico`);
    }
  });
});
