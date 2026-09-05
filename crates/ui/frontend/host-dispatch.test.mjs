import { test } from "node:test";
import assert from "node:assert/strict";
import { classifyServerMsg } from "./host-dispatch.mjs";

test("messaggi che aprono finestre sono classificati window", () => {
  for (const type of ["open_window", "search_open", "open_plugin_window", "update_plugin_window", "close_plugin_window", "routine_save_preview", "open_output_window", "output_window_content", "open_ui_local", "ui_ping"]) {
    assert.equal(classifyServerMsg({ type }), "window");
  }
});
test("messaggi AI Chat / note / share sono relay", () => {
  for (const type of ["ai_chat_message", "ai_chat_roster", "notes_snapshot", "note_upserted", "share_request", "share_result", "share_content_request", "share_incoming_data", "ai_chat_join_prompt"]) {
    assert.equal(classifyServerMsg({ type }), "relay");
  }
});
test("messaggi del cursore v1 senza superficie sono ignored", () => {
  for (const type of ["chunk", "heartbeat", "done", "error", "pong", "cwd", "server_info"]) {
    assert.equal(classifyServerMsg({ type }), "ignored");
  }
});
test("tool_confirm_request senza cursore viene negato (safe default)", () => {
  assert.equal(classifyServerMsg({ type: "tool_confirm_request" }), "deny");
});
