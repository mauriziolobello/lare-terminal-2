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

test("Parità chiavi i18n: tutti i file di lingua sono sincronizzati con it.json e coprono tutte le chiavi usate", () => {
  const itFile = path.join(i18nDir, "it.json");
  assert.ok(fs.existsSync(itFile), `File di riferimento non trovato: ${itFile}`);

  const itDict = JSON.parse(fs.readFileSync(itFile, "utf-8"));
  const itKeys = Object.keys(itDict).sort();

  // Trova tutti i file *.json nella cartella i18n escluso it.json
  const otherJsonFiles = fs
    .readdirSync(i18nDir)
    .filter((f) => f.endsWith(".json") && f !== "it.json");

  assert.ok(
    otherJsonFiles.length > 0,
    "Almeno una lingua aggiuntiva oltre a 'it' deve essere presente in i18nDir"
  );

  // 1. Verifica che ogni dizionario aggiuntivo abbia esattamente le stesse chiavi di it.json
  for (const file of otherJsonFiles) {
    const langFilePath = path.join(i18nDir, file);
    const langDict = JSON.parse(fs.readFileSync(langFilePath, "utf-8"));
    const langKeys = Object.keys(langDict).sort();

    const missingInLang = itKeys.filter((k) => !(k in langDict));
    const missingInIt = langKeys.filter((k) => !(k in itDict));

    assert.deepEqual(
      missingInLang,
      [],
      `Chiavi presenti in it.json ma mancanti in ${file}: ${missingInLang.join(", ")}`
    );
    assert.deepEqual(
      missingInIt,
      [],
      `Chiavi presenti in ${file} ma mancanti in it.json: ${missingInIt.join(", ")}`
    );
  }

  // 2. Verifica che ogni chiave usata nel codice esista in it.json
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
