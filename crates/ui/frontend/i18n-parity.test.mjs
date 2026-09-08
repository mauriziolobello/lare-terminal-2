import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// Radice repository e cartelle di scansione
const repoRoot = path.resolve(__dirname, "../../..");
const frontendDir = path.resolve(__dirname);
const rustSrcDir = path.resolve(repoRoot, "crates/ui/src-tauri/src");
const i18nDir = path.resolve(repoRoot, "Test Run/Configuration/i18n");

/**
 * Raccoglie tutti i file ricorsivamente con le estensioni fornite,
 * escludendo cartelle/file ignorati.
 */
function collectFiles(dir, extensions, isExcluded) {
  const results = [];
  if (!fs.existsSync(dir)) return results;

  const entries = fs.readdirSync(dir, { withFileTypes: true });
  for (const entry of entries) {
    const fullPath = path.join(dir, entry.name);
    if (isExcluded(fullPath, entry.name)) continue;

    if (entry.isDirectory()) {
      results.push(...collectFiles(fullPath, extensions, isExcluded));
    } else if (entry.isFile()) {
      if (extensions.some((ext) => entry.name.endsWith(ext))) {
        results.push(fullPath);
      }
    }
  }
  return results;
}

/**
 * Estrae tutte le chiavi i18n utilizzate nei file sorgente del frontend e del backend Rust.
 */
function extractUsedKeys() {
  const usedKeys = new Set();

  // 1. Frontend: file .html, .js, .mjs (esclusi vendor e test)
  const frontendFiles = collectFiles(
    frontendDir,
    [".html", ".js", ".mjs"],
    (fullPath, name) => {
      if (name === "vendor") return true;
      if (name.includes(".test.")) return true;
      return false;
    }
  );

  // Regex per catturare chiavi letterali:
  // - t("chiave") o t('chiave')
  // - data-i18n="chiave"
  // - data-i18n-placeholder="chiave"
  // - data-i18n-title="chiave"
  // - data-i18n-aria-label="chiave"
  const tRegex = /\bt\(\s*["']([^"']+)["']/g;
  const dataI18nRegex = /data-i18n(?:-[a-z-]+)?=["']([^"']+)["']/g;

  for (const file of frontendFiles) {
    const content = fs.readFileSync(file, "utf-8");
    for (const match of content.matchAll(tRegex)) {
      usedKeys.add(match[1]);
    }
    for (const match of content.matchAll(dataI18nRegex)) {
      usedKeys.add(match[1]);
    }
  }

  // 2. Rust: t_sync(..., "chiave") nel codice di produzione (escluso #[cfg(test)])
  const rustFiles = collectFiles(
    rustSrcDir,
    [".rs"],
    (fullPath, name) => false
  );

  const rustTSyncRegex = /t_sync\([^,]+,[^,]+,\s*["']([^"']+)["']/g;
  for (const file of rustFiles) {
    let content = fs.readFileSync(file, "utf-8");
    const testIdx = content.indexOf("#[cfg(test)]");
    if (testIdx !== -1) {
      content = content.slice(0, testIdx);
    }
    for (const match of content.matchAll(rustTSyncRegex)) {
      usedKeys.add(match[1]);
    }
  }

  return usedKeys;
}

test("Parità chiavi i18n: it.json ed en.json sono sincronizzati e coprono tutte le chiavi usate", () => {
  const itFile = path.join(i18nDir, "it.json");
  const enFile = path.join(i18nDir, "en.json");

  assert.ok(fs.existsSync(itFile), `File non trovato: ${itFile}`);
  assert.ok(fs.existsSync(enFile), `File non trovato: ${enFile}`);

  const itDict = JSON.parse(fs.readFileSync(itFile, "utf-8"));
  const enDict = JSON.parse(fs.readFileSync(enFile, "utf-8"));

  const itKeys = Object.keys(itDict).sort();
  const enKeys = Object.keys(enDict).sort();

  // 1. Verifica che it.json ed en.json abbiano esattamente le stesse chiavi
  const missingInEn = itKeys.filter((k) => !(k in enDict));
  const missingInIt = enKeys.filter((k) => !(k in itDict));

  assert.deepEqual(
    missingInEn,
    [],
    `Chiavi presenti in it.json ma mancanti in en.json: ${missingInEn.join(", ")}`
  );
  assert.deepEqual(
    missingInIt,
    [],
    `Chiavi presenti in en.json ma mancanti in it.json: ${missingInIt.join(", ")}`
  );

  // 2. Verifica che ogni chiave usata nel codice esista in it.json ed en.json
  const usedKeys = extractUsedKeys();
  const itKeySet = new Set(itKeys);

  const missingFromDict = Array.from(usedKeys).filter((k) => !itKeySet.has(k));
  assert.deepEqual(
    missingFromDict,
    [],
    `Chiavi usate nel codice ma non definite nei dizionari i18n: ${missingFromDict.join(", ")}`
  );

  // 3. Verifica che ogni chiave presente nei dizionari sia usata nel codice
  const unusedInCode = itKeys.filter((k) => !usedKeys.has(k));
  assert.deepEqual(
    unusedInCode,
    [],
    `Chiavi definite nei dizionari ma non utilizzate nel codice: ${unusedInCode.join(", ")}`
  );
});
