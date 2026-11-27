// Port of native/src/services/route_protocol_bridge/turn_reminder.rs
"use strict";
var DEFAULT_REMINDER = "请用简体中文回复。";

function lastRoleIsUser(list, userRole) {
  if (!Array.isArray(list) || list.length === 0) return false;
  var last = list[list.length - 1];
  return !!last && typeof last === "object" && last.role === userRole;
}

function appendAnthropic(body, reminder) {
  if (!Array.isArray(body.messages) || !lastRoleIsUser(body.messages, "user")) return false;
  var last = body.messages[body.messages.length - 1];
  if (typeof last.content === "string") last.content = [{ type: "text", text: last.content }];
  if (!Array.isArray(last.content)) return false;
  last.content.push({ type: "text", text: reminder });
  return true;
}

function appendChat(body, reminder) {
  if (!Array.isArray(body.messages) || !lastRoleIsUser(body.messages, "user")) return false;
  var last = body.messages[body.messages.length - 1];
  if (typeof last.content === "string") {
    last.content = last.content + "\n\n" + reminder;
    return true;
  }
  if (Array.isArray(last.content)) {
    last.content.push({ type: "text", text: reminder });
    return true;
  }
  return false;
}

function appendResponses(body, reminder) {
  if (typeof body.input === "string") {
    body.input = body.input + "\n\n" + reminder;
    return true;
  }
  if (!Array.isArray(body.input) || !lastRoleIsUser(body.input, "user")) return false;
  var last = body.input[body.input.length - 1];
  if (typeof last.content === "string") last.content = [{ type: "input_text", text: last.content }];
  if (!Array.isArray(last.content)) return false;
  last.content.push({ type: "input_text", text: reminder });
  return true;
}

function appendGemini(body, reminder) {
  if (!Array.isArray(body.contents) || !lastRoleIsUser(body.contents, "user")) return false;
  var last = body.contents[body.contents.length - 1];
  if (!Array.isArray(last.parts)) return false;
  last.parts.push({ text: reminder });
  return true;
}

function onRequest(body, ctx) {
  if (!body || typeof body !== "object" || Array.isArray(body)) return null;
  var config = (ctx && ctx.config) || {};
  var reminder = (typeof config.text === "string" ? config.text.trim() : "") || DEFAULT_REMINDER;
  if (reminder === "") return null;
  var serialized = JSON.stringify(body);
  if (serialized.indexOf(reminder) >= 0) return null;
  var dialect = (ctx && ctx.dialect) || "openai";
  var appended = false;
  if (dialect === "anthropic") appended = appendAnthropic(body, reminder);
  else if (dialect === "openai-responses" || dialect === "openai_responses" || dialect === "responses") appended = appendResponses(body, reminder);
  else if (dialect === "gemini") appended = appendGemini(body, reminder);
  else appended = appendChat(body, reminder);
  return appended ? body : null;
}

