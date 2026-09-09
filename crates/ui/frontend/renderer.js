// renderer.js — DOM rendering for Lare Terminal output.
//
// Responsibility (SRP): owns ONLY how ServerMsg data is displayed.
// It does NOT touch the WebSocket; it receives plain objects from ws-client.js.
import { t } from "./i18n.mjs";


const MAX_OUTPUT_LINES = 500; // prevent unbounded growth in long sessions

/**
 * LareRenderer
 *
 * Manages the output area: appends chunks, marks done/error, scrolls.
 *
 * Usage:
 *   const r = new LareRenderer(outputEl, statusEl);
 *   r.appendChunk("cmd-1", "line of output\n");
 *   r.markDone("cmd-1", 0);
 *   r.markError("cmd-1", "os_error", "command not found");
 *   r.setStatus("connected");
 */
export class LareRenderer {
  /**
   * @param {HTMLElement} outputEl  - The scrollable output container.
   * @param {HTMLElement} statusEl  - The small connection-status indicator.
   */
  constructor(outputEl, statusEl) {
    this._output = outputEl;
    this._statusEl = statusEl;
    /** @type {Map<string, HTMLElement>} command-id → DOM block */
    this._blocks = new Map();
    /** @type {Map<string, string>} raw accumulated text per command (for Markdown render-on-done) */
    this._rawText = new Map();
  }

  // ── Output area ────────────────────────────────────────────────────────

  /**
   * Append streaming chunk content for a command.
   * Creates a new block the first time a command id is seen.
   *
   * @param {string} id
   * @param {string} content
   */
  appendChunk(id, content) {
    const block = this._getOrCreateBlock(id);
    const pre = block.querySelector("pre");
    if (pre) {
      pre.textContent += content;
      // Buffer raw text separately; pre.textContent normalises whitespace, so
      // marked needs the original string to produce correct Markdown output.
      this._rawText.set(id, (this._rawText.get(id) ?? "") + content);
      this._scrollToBottom();
      this._trimOldLines();
    }
  }

  /**
   * Echo a just-submitted user command into the output area as a distinct
   * block, so the user immediately sees what they typed (and that it was
   * received) even before any response arrives.
   *
   * Not keyed by command id: a standalone visual marker shown above the
   * eventual response block.  Uses createElement/textContent (never innerHTML)
   * so the echoed text cannot inject markup.
   *
   * @param {string} text  The submitted input (already trimmed).
   */
  echoCommand(text) {
    const block = document.createElement("div");
    block.className = "cmd-echo";
    const pre = document.createElement("pre");
    // Prefix with the prompt glyph so it reads like a shell echo.
    pre.textContent = `› ${text}`;
    block.appendChild(pre);
    this._output.appendChild(block);
    this._scrollToBottom();
    this._trimOldLines();
  }

  /**
   * Append a standalone system message — not tied to any command id, not an
   * echo of user input. Used for async notifications the user didn't just
   * type (e.g. a Share outcome arriving up to 24h after the request).
   *
   * Uses createElement/textContent (never innerHTML) — same DOM-safety
   * invariant as the rest of this file.
   *
   * @param {string} text
   */
  systemMessage(text) {
    const block = document.createElement("div");
    block.className = "sys-msg";
    const pre = document.createElement("pre");
    pre.textContent = text;
    block.appendChild(pre);
    this._output.appendChild(block);
    this._scrollToBottom();
    this._trimOldLines();
  }

  /**
   * Append an interactive confirmation banner for a gated AI tool (local
   * tool-confirm gate — Docs/superpowers/specs/2026-07-15-local-tool-confirm-gate-design.md).
   * Shows `commands` (multi-line label, same shape as the Telegram gate prompt)
   * with [Esegui]/[Annulla] buttons; calls `onDecision(accept)` exactly once,
   * then disables both buttons so a second click can't send a duplicate response.
   *
   * @param {string} id                          - opaque id from ServerMsg::ToolConfirmRequest
   * @param {string} commands
   * @param {(accept: boolean) => void} onDecision
   */
  confirmBanner(id, commands, onDecision) {
    const block = document.createElement("div");
    block.className = "confirm-banner";
    block.dataset.confirmId = id;

    const pre = document.createElement("pre");
    pre.textContent = commands;
    block.appendChild(pre);

    const buttons = document.createElement("div");
    buttons.className = "confirm-buttons";

    let decided = false;
    const decide = (accept) => {
      if (decided) return;
      decided = true;
      okBtn.disabled = true;
      noBtn.disabled = true;
      onDecision(accept);
    };

    const okBtn = document.createElement("button");
    okBtn.textContent = t("common.execute");
    okBtn.addEventListener("click", () => decide(true));

    const noBtn = document.createElement("button");
    noBtn.textContent = t("common.cancel");
    noBtn.addEventListener("click", () => decide(false));

    buttons.appendChild(okBtn);
    buttons.appendChild(noBtn);
    block.appendChild(buttons);

    this._output.appendChild(block);
    this._scrollToBottom();
    this._trimOldLines();
  }

  /**
   * Finalize a command block with exit code.
   *
   * @param {string}      id
   * @param {number|null} exitCode
   */
  markDone(id, exitCode) {
    const block = this._getOrCreateBlock(id);
    // exitCode null/undefined → AI response: render accumulated text as Markdown.
    if (exitCode === null || exitCode === undefined) {
      this._renderAsMarkdown(id, block);
    }
    this._rawText.delete(id);
    block.classList.add("done");
    if (exitCode !== null && exitCode !== undefined) {
      const badge = document.createElement("span");
      badge.className = exitCode === 0 ? "exit-ok" : "exit-err";
      badge.textContent = `exit ${exitCode}`;
      block.appendChild(badge);
    }
    this._scrollToBottom();
  }

  /**
   * Mark a command block as errored.
   *
   * @param {string} id
   * @param {string} code     - ErrCode snake_case string
   * @param {string} message
   */
  markError(id, code, message) {
    const block = this._getOrCreateBlock(id);
    this._rawText.delete(id);
    block.classList.add("error");
    const pre = block.querySelector("pre");
    if (pre) {
      pre.textContent += `\n[${code}] ${message}`;
    }
    this._scrollToBottom();
  }

  /** Clear all output blocks (safe DOM removal — no innerHTML). */
  clear() {
    // Remove children one by one to avoid innerHTML with any content.
    while (this._output.firstChild) {
      this._output.removeChild(this._output.firstChild);
    }
    this._blocks.clear();
    this._rawText.clear();
  }

  // ── Connection status indicator ────────────────────────────────────────

  /**
   * Update the visual status badge.
   *
   * @param {"connecting"|"connected"|"disconnected"|"error"|"failed"} status
   */
  setStatus(status) {
    if (!this._statusEl) return;
    const labels = {
      connecting: t("ext_channel.status_connecting"),
      connected:  t("ext_channel.status_connected"),
      disconnected: t("ext_channel.status_disconnected"),
      error:      t("ext_channel.status_error"),
      failed:     t("ext_channel.status_failed"),
    };
    this._statusEl.textContent = labels[status] ?? status;
    this._statusEl.className = `status status-${status}`;
  }

  // ── Private helpers ────────────────────────────────────────────────────

  /** Return the command block for `id`, creating it if absent. */
  _getOrCreateBlock(id) {
    if (this._blocks.has(id)) return this._blocks.get(id);

    const block = document.createElement("div");
    block.className = "cmd-block";
    block.dataset.cmdId = id;

    const pre = document.createElement("pre");
    block.appendChild(pre);

    this._output.appendChild(block);
    this._blocks.set(id, block);
    return block;
  }

  /**
   * Replace the <pre> of a command block with Markdown-rendered HTML.
   * No-ops silently if marked/DOMPurify are not loaded or text is empty.
   *
   * @param {string}      id
   * @param {HTMLElement} block
   */
  _renderAsMarkdown(id, block) {
    if (!window.marked || !window.DOMPurify) return;
    const raw = this._rawText.get(id) ?? "";
    if (!raw.trim()) return;
    const pre = block.querySelector("pre");
    if (!pre) return;
    // breaks: true → single \n becomes <br>; appropriate for terminal-style text
    // where the LLM echoes a command on one line and the explanation on the next.
    const safeHtml = window.DOMPurify.sanitize(window.marked.parse(raw, { breaks: true }));
    const mdDiv = document.createElement("div");
    mdDiv.className = "md-output";
    mdDiv.innerHTML = safeHtml;
    block.replaceChild(mdDiv, pre);
  }

  _scrollToBottom() {
    this._output.scrollTop = this._output.scrollHeight;
  }

  /** Remove oldest blocks if the output element has too many child nodes. */
  _trimOldLines() {
    while (this._output.children.length > MAX_OUTPUT_LINES) {
      const first = this._output.firstElementChild;
      if (first) {
        const id = first.dataset.cmdId;
        this._output.removeChild(first);
        if (id) this._blocks.delete(id);
      } else {
        break;
      }
    }
  }
}
