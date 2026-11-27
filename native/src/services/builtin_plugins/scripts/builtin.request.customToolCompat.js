// Port of rewrite_custom_tools_in_responses_request in route_proxy_service.rs
// Rewrites Responses custom tools into function tools for upstreams lacking
// native custom tool support.
"use strict";
var INPUT_FIELD = "input";
var INPUT_DESCRIPTION = "Raw string input for the original custom tool. Preserve formatting exactly and follow the original tool definition embedded in the description.";
var META_HEADING = "Original tool definition:";

function toolType(value) {
  return value && typeof value === "object" && !Array.isArray(value) && typeof value.type === "string" ? value.type : null;
}

function trimString(value) {
  return typeof value === "string" && value.trim() !== "" ? value.trim() : null;
}

function toolName(tool) {
  return trimString(tool.name) || trimString(tool.function && tool.function.name);
}

function customToolToFunction(tool, name) {
  var description = trimString(tool.description);
  if (!description) {
    description = META_HEADING + "\n```json\n" + JSON.stringify(tool) + "\n```";
  }
  var params = { type: "object", properties: {}, required: [INPUT_FIELD], additionalProperties: false };
  params.properties[INPUT_FIELD] = { type: "string", description: INPUT_DESCRIPTION };
  return { type: "function", name: name, description: description, parameters: params };
}

function callToFunction(item) {
  var out = { type: "function_call" };
  ["id", "call_id", "status", "name"].forEach(function (key) {
    if (Object.prototype.hasOwnProperty.call(item, key)) out[key] = item[key];
  });
  var input = Object.prototype.hasOwnProperty.call(item, "input") ? item.input : "";
  var wrapped = {};
  wrapped[INPUT_FIELD] = input;
  out.arguments = JSON.stringify(wrapped);
  return out;
}

function outputToFunctionOutput(item) {
  var out = { type: "function_call_output" };
  ["call_id", "id"].forEach(function (key) {
    if (Object.prototype.hasOwnProperty.call(item, key)) out[key] = item[key];
  });
  if (Object.prototype.hasOwnProperty.call(item, "output")) out.output = item.output;
  else if (Object.prototype.hasOwnProperty.call(item, "input")) out.output = item.input;
  return out;
}

function onRequest(body, ctx) {
  if (!body || typeof body !== "object" || Array.isArray(body)) return null;
  var changed = false;
  if (Array.isArray(body.tools)) {
    body.tools = body.tools.map(function (tool) {
      if (toolType(tool) !== "custom") return tool;
      var name = toolName(tool);
      if (!name) return tool;
      changed = true;
      return customToolToFunction(tool, name);
    });
  }
  if (Array.isArray(body.input)) {
    body.input = body.input.map(function (item) {
      var kind = toolType(item);
      if (kind === "custom_tool_call") { changed = true; return callToFunction(item); }
      if (kind === "custom_tool_call_output") { changed = true; return outputToFunctionOutput(item); }
      return item;
    });
  }
  return changed ? body : null;
}
