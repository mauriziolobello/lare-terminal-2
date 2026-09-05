// cwd-format.js — pure function for formatting the current working directory.
//
// SRP: no DOM, no I/O. Given a path string, a home string, and an optional
// maximum display length, returns the display string. Testable with node:test.

/**
 * Normalize path separators to forward slashes for uniform comparison.
 * @param {string} p
 * @returns {string}
 */
function normalize(p) {
  return p.replace(/\\/g, "/");
}

/**
 * Return the last path segment (after the last "/" in the normalized form).
 * @param {string} p — already normalized (forward slashes)
 * @returns {string}
 */
function lastSegment(p) {
  const idx = p.lastIndexOf("/");
  return idx === -1 ? p : p.slice(idx + 1);
}

/**
 * Format a cwd path for compact display.
 *
 * 1. If `path` starts with `home` (non-empty), replace that prefix with `~`.
 *    Both `\\` and `/` are treated as equivalent separators for the comparison
 *    and for the output (output uses `/` as separator).
 * 2. If the resulting string length exceeds `maxLen`, apply a middle ellipsis:
 *    keep an initial prefix and the last path segment, joined with `...`.
 *    The result is guaranteed to be <= maxLen characters (when possible; if
 *    the last segment alone exceeds maxLen the last segment is returned as-is).
 * 3. Otherwise return the string unchanged.
 *
 * @param {string} path   — the raw cwd path from the OS
 * @param {string} home   — the user's home directory (pass "" to disable ~ substitution)
 * @param {number} [maxLen=48]
 * @returns {string}
 */
export function formatCwd(path, home, maxLen = 48) {
  let display = normalize(path);
  const normHome = normalize(home);

  // Replace home prefix with ~ (only when home is non-empty).
  if (normHome !== "" && display.startsWith(normHome)) {
    const rest = display.slice(normHome.length);
    // rest may start with "/" (e.g. "/projects") or be empty (path === home).
    display = rest.startsWith("/") ? "~" + rest : "~";
  }

  // Short enough — return as-is.
  if (display.length <= maxLen) {
    return display;
  }

  // Middle ellipsis: preserve the last segment and as much of the start as fits.
  const last = lastSegment(display);
  const suffix = ".../" + last; // e.g. ".../segment"

  // Budget for the prefix: maxLen minus the suffix length minus one separator.
  const prefixBudget = maxLen - suffix.length;

  if (prefixBudget <= 0) {
    // Last segment alone is the best we can do.
    return last;
  }

  // Find a clean cut point (at a "/" boundary) within the budget.
  let cut = prefixBudget;
  // Walk backwards to avoid cutting inside a segment name.
  while (cut > 0 && display[cut - 1] !== "/") {
    cut--;
  }
  // If no "/" found within budget fall back to a hard character cut.
  if (cut === 0) {
    cut = prefixBudget;
  }

  const prefix = display.slice(0, cut).replace(/\/$/, ""); // strip trailing /
  return prefix + "/" + suffix;
}
