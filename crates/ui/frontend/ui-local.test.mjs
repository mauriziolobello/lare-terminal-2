import { test } from "node:test";
import assert from "node:assert/strict";
import { resolveUiLocal, markdownWindowLabel, outputWindowIdFromLabel } from "./ui-local.mjs";
import { EXTERNAL_TOOL_CHANNELS } from "./external-channels.js";

test("open_ui_local: i tre singleton locali mappano sui comandi Tauri esistenti", () => {
  assert.deepEqual(resolveUiLocal("config", EXTERNAL_TOOL_CHANNELS), { cmd: "open_config_window", args: undefined });
  assert.deepEqual(resolveUiLocal("library", EXTERNAL_TOOL_CHANNELS), { cmd: "open_library_window", args: undefined });
  assert.deepEqual(resolveUiLocal("aichat", EXTERNAL_TOOL_CHANNELS), { cmd: "open_aichat_window", args: undefined });
});

test("open_ui_local: un id di canale esterno apre la sua finestra col titolo della tabella", () => {
  assert.deepEqual(resolveUiLocal("nmap", EXTERNAL_TOOL_CHANNELS), {
    cmd: "open_external_channel_window",
    args: { channelId: "nmap", windowTitle: "Lare — nmap" },
  });
  assert.deepEqual(resolveUiLocal("financial-markets", EXTERNAL_TOOL_CHANNELS).args.channelId, "financial-markets");
});

test("open_ui_local: nome ignoto → null (nessun comando inventato)", () => {
  assert.equal(resolveUiLocal("boh", EXTERNAL_TOOL_CHANNELS), null);
  assert.equal(resolveUiLocal("", EXTERNAL_TOOL_CHANNELS), null);
});

test("/help è singleton (label fissa), le altre finestre Markdown no", () => {
  assert.equal(markdownWindowLabel("help"), "help");
  assert.equal(markdownWindowLabel("markdown"), null);
  assert.equal(markdownWindowLabel(undefined), null);
});

test("window_id dalla label di una finestra di output", () => {
  assert.equal(outputWindowIdFromLabel("output-c1"), "c1");
  assert.equal(outputWindowIdFromLabel("output-abc-123"), "abc-123");
  assert.equal(outputWindowIdFromLabel("md-1-2"), null);
  assert.equal(outputWindowIdFromLabel(undefined), null);
});
