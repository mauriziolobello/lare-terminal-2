// path-utils.js — pure path helpers for the frontend. No DOM, no I/O.

/**
 * Directory padre di `path`, gestendo separatori `\` e `/`.
 * - nessun separatore → ritorna `path` invariato;
 * - radice del drive ("C:\file") → "C:\"; root POSIX ("/file") → "/".
 * @param {string} path
 * @returns {string}
 */
export function parentDir(path) {
  if (typeof path !== "string" || path.length === 0) return path;
  const idx = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  if (idx < 0) return path;                 // no separator
  if (idx === 0) return path.slice(0, 1);   // POSIX root "/file" → "/"
  const parent = path.slice(0, idx);
  // Drive root: "C:" → "C:\" (so the file manager opens the drive).
  if (/^[A-Za-z]:$/.test(parent)) return parent + "\\";
  return parent;
}

/**
 * Bounds of the "word" touching `caret` in `text` — used for Tab-completion
 * (Docs/superpowers/... tab-path-completion, 2026-07-19): the word runs from
 * the nearest whitespace (or start of string) up to `caret` itself. Anything
 * AFTER the caret on the same word is deliberately left alone — this mirrors
 * standard shell completion behavior (completes up to the cursor).
 *
 * @param {string} text
 * @param {number} caret
 * @returns {{ start: number, end: number }}
 */
export function wordBounds(text, caret) {
  let start = caret;
  while (start > 0 && !/\s/.test(text[start - 1])) start--;
  return { start, end: caret };
}

/**
 * True if the word starting at `wordStart` is an ARGUMENT (there is at
 * least one earlier, non-whitespace word before it on the line) rather than
 * the command word itself (position 0, ignoring leading whitespace).
 * Tab-completion only fires for argument positions — the command word is a
 * cmdlet/executable name, not a file path, and completing it as one would
 * be wrong (out of scope for this feature; see the design writeup).
 *
 * @param {string} text
 * @param {number} wordStart
 * @returns {boolean}
 */
export function isArgumentPosition(text, wordStart) {
  let i = wordStart;
  while (i > 0 && /\s/.test(text[i - 1])) i--;
  return i > 0;
}

/**
 * Splits a path-like token into its directory part and the partial name
 * being completed. `dirPart` includes the trailing separator (empty string
 * if `token` has no separator at all, meaning "relative to cwd, no
 * subdirectory typed yet"). Mirrors `parentDir`'s separator handling
 * (`\` and `/` both recognized) but keeps the separator attached to
 * `dirPart` rather than stripping it, since the caller re-inserts `dirPart`
 * verbatim (untouched) and only replaces `prefix`.
 *
 * `quoteChar` is `"` or `'` when the token starts with that quote character
 * (e.g. the user typed `cd "My` to handle a name with spaces), `null`
 * otherwise. The leading quote is stripped from `dirPart`/`prefix` — a
 * literal quote character never matches a real filename — and returned
 * separately so the caller can re-wrap the eventual candidate in a matching
 * quote pair (2026-07-19 fix: the leading quote used to be treated as an
 * ordinary token character, so `prefix` for `"My` was the string `"My`
 * itself — no real filename starts with `"`, so nothing ever matched).
 * Only a LEADING quote is handled; a trailing quote the user already typed
 * is out of scope here.
 *
 * @param {string} token
 * @returns {{ dirPart: string, prefix: string, quoteChar: string | null }}
 */
export function splitPathToken(token) {
  let quoteChar = null;
  let rest = token;
  if (rest.length > 0 && (rest[0] === '"' || rest[0] === "'")) {
    quoteChar = rest[0];
    rest = rest.slice(1);
  }
  const idx = Math.max(rest.lastIndexOf("\\"), rest.lastIndexOf("/"));
  if (idx < 0) return { dirPart: "", prefix: rest, quoteChar };
  return { dirPart: rest.slice(0, idx + 1), prefix: rest.slice(idx + 1), quoteChar };
}

/**
 * Replaces `text[rangeStart..rangeEnd)` with `replacement`, returning a new
 * `{ text, caret }` pair (same shape as `line-editor.js`'s editor state)
 * with the caret placed right after the inserted text. Used both for the
 * initial Tab-completion insertion (`rangeEnd` = the original caret) and for
 * cycling through further candidates (`rangeEnd` = the end of the
 * previously-inserted candidate).
 *
 * @param {string} text
 * @param {number} rangeStart
 * @param {number} rangeEnd
 * @param {string} replacement
 * @returns {{ text: string, caret: number }}
 */
export function replaceRange(text, rangeStart, rangeEnd, replacement) {
  return {
    text: text.slice(0, rangeStart) + replacement + text.slice(rangeEnd),
    caret: rangeStart + replacement.length,
  };
}

/**
 * Builds the final text to insert for a Tab-completion candidate:
 * re-prepends `dirPart` (completion candidates are always bare basenames —
 * the source never echoes `dirPart` back) and wraps the whole thing in
 * `quoteChar` if the original token had a leading quote (`null` when it
 * didn't). Without re-prepending `dirPart` here, a typed subdirectory
 * silently vanishes from the line on completion — found in review,
 * 2026-07-19: `cd Sub\Fi<Tab>` was completing to `cd File.txt` instead of
 * `cd Sub\File.txt`, because `replaceRange` replaces the ENTIRE token span
 * (which includes `dirPart`) with whatever string is passed as the
 * replacement — passing the bare `candidate` alone dropped `dirPart`.
 *
 * @param {string} dirPart
 * @param {string} candidate
 * @param {string | null} quoteChar
 * @returns {string}
 */
export function buildCompletionText(dirPart, candidate, quoteChar) {
  const body = `${dirPart}${candidate}`;
  return quoteChar ? `${quoteChar}${body}${quoteChar}` : body;
}
