import { test } from "node:test";
import assert from "node:assert/strict";
import { initI18n, getDict, t, fetchI18n, applyI18n } from "./i18n.mjs";

test("t(key) restituisce la traduzione se presente", () => {
  initI18n({ "common.save": "Salva", "config.title": "Configurazione" });
  assert.equal(t("common.save"), "Salva");
  assert.equal(t("config.title"), "Configurazione");
});

test("t(key) fallback: restituisce la chiave se assente", () => {
  initI18n({ "common.save": "Salva" });
  assert.equal(t("missing.key"), "missing.key");
});

test("t(key, params) interpola parametri {param}", () => {
  initI18n({
    "greeting": "Ciao {name}!",
    "routine.status": "Routine {name} ha {count} elementi.",
  });
  assert.equal(t("greeting", { name: "Maurizio" }), "Ciao Maurizio!");
  assert.equal(t("routine.status", { name: "backup", count: 3 }), "Routine backup ha 3 elementi.");
});

test("t(key) con input non stringa restituisce stringa vuota", () => {
  assert.equal(t(null), "");
  assert.equal(t(undefined), "");
  assert.equal(t(123), "");
});

test("initI18n e getDict gestiscono copie isolate", () => {
  initI18n({ a: "1" });
  const d = getDict();
  assert.deepEqual(d, { a: "1" });
  d.a = "2";
  assert.equal(t("a"), "1"); // non mutato dall'esterno
});

test("fetchI18n invoca il comando Tauri get_i18n e popola il dizionario", async () => {
  let invokedWith = null;
  const mockInvoke = async (cmd, args) => {
    invokedWith = { cmd, args };
    return { "common.save": "Save", "common.cancel": "Cancel" };
  };

  const res = await fetchI18n(mockInvoke, "en");
  assert.deepEqual(invokedWith, { cmd: "get_i18n", args: { lang: "en" } });
  assert.equal(t("common.save"), "Save");
  assert.equal(t("common.cancel"), "Cancel");
  assert.deepEqual(res, { "common.save": "Save", "common.cancel": "Cancel" });
});

test("fetchI18n è infallibile su eccezione di invoke", async () => {
  initI18n({ "prev.key": "val" });
  const failingInvoke = async () => {
    throw new Error("IPC error");
  };
  const res = await fetchI18n(failingInvoke, "it");
  assert.equal(t("prev.key"), "val");
  assert.deepEqual(res, { "prev.key": "val" });
});

test("applyI18n traduce attributi data-i18n nel DOM", () => {
  initI18n({
    "title.key": "Mio Titolo",
    "placeholder.key": "Digita qui...",
    "tooltip.key": "Informazioni aggiuntive",
    "aria.key": "Descrizione accessibile",
  });

  // Mock minimale del DOM compatibile con querySelectorAll
  class MockElement {
    constructor(attrs = {}) {
      this._attrs = attrs;
      this.textContent = "";
    }
    getAttribute(name) {
      return this._attrs[name] ?? null;
    }
    setAttribute(name, val) {
      this._attrs[name] = val;
    }
  }

  const elText = new MockElement({ "data-i18n": "title.key" });
  const elPh = new MockElement({ "data-i18n-placeholder": "placeholder.key" });
  const elTitle = new MockElement({ "data-i18n-title": "tooltip.key" });
  const elAria = new MockElement({ "data-i18n-aria-label": "aria.key" });

  const mockContainer = {
    querySelectorAll(selector) {
      if (selector === "[data-i18n]") return [elText];
      if (selector === "[data-i18n-placeholder]") return [elPh];
      if (selector === "[data-i18n-title]") return [elTitle];
      if (selector === "[data-i18n-aria-label]") return [elAria];
      return [];
    },
  };

  applyI18n(mockContainer);

  assert.equal(elText.textContent, "Mio Titolo");
  assert.equal(elPh.getAttribute("placeholder"), "Digita qui...");
  assert.equal(elTitle.getAttribute("title"), "Informazioni aggiuntive");
  assert.equal(elAria.getAttribute("aria-label"), "Descrizione accessibile");
});
