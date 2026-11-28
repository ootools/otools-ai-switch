import { useState } from "react";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { PipelineFlowEditor } from "../src/components/PipelineFlowEditor";
import type { BuiltinPluginDescriptor, PipelineConfig } from "../src/lib/api/types";

const catalog: BuiltinPluginDescriptor[] = [
  { id: "compression", name: "请求压缩", description: "压缩请求体，减少传输开销。", lane: "request", kind: "transform", config_fields: [{ key: "mode", label: "模式", kind: "select" }] },
  { id: "retry", name: "重试策略", description: "请求失败后按设定次数重试。", lane: "request", kind: "schedule", config_fields: [
    { key: "count", label: "重试次数", kind: "number" },
    { key: "interval", label: "重试间隔(ms)", kind: "number" },
    { key: "elapsed", label: "总时长上限(秒)", kind: "number" },
  ] },
  { id: "guard", name: "内容检查", description: "检查请求内容。", lane: "request", kind: "transform", config_fields: [
    { key: "strict", label: "严格检查", kind: "bool" },
    { key: "note", label: "提示词", kind: "textarea" },
  ] },
  { id: "cleanup", name: "响应清理", description: "移除不兼容的响应内容。", lane: "response", kind: "transform", config_fields: [{ key: "mode", label: "模式", kind: "select" }] },
  { id: "pack:custom", name: "自定义插件", description: "第三方中间件。", lane: "both", kind: "transform", config_fields: [] },
];

function fixture(): PipelineConfig {
  return {
    version: 1,
    request: { mode: "process", chain: [
      { id: "compression", enabled: true, config: { mode: "auto", vendor: { keep: true } } },
      { id: "retry", enabled: false, config: { count: 2, interval: 200, elapsed: 0, custom: [1, 2] } },
      { id: "guard", enabled: true, config: { strict: false, note: "保留原始内容" } },
      { id: "missing.plugin", enabled: false, config: { opaque: { items: ["keep"] } } },
    ] },
    response: { mode: "process", chain: [{ id: "cleanup", enabled: true, config: { mode: "on" } }] },
  };
}

function Harness({ initial = fixture(), entries = catalog }: { initial?: PipelineConfig; entries?: BuiltinPluginDescriptor[] }) {
  const [pipeline, setPipeline] = useState(initial);
  return <>
    <PipelineFlowEditor pipeline={pipeline} catalog={entries} onChange={setPipeline} />
    <output data-testid="pipeline-value">{JSON.stringify(pipeline)}</output>
  </>;
}

function current(): PipelineConfig {
  return JSON.parse(screen.getByTestId("pipeline-value").textContent ?? "{}");
}

function pluginNames() {
  return within(screen.getByRole("list", { name: "请求方向插件" })).getAllByRole("listitem").map((row) => row.textContent);
}

describe("PipelineFlowEditor compact settings", () => {
  it("shows one direction with ordered collapsed rows and parameter summaries", () => {
    render(<Harness />);
    expect(screen.getByRole("tab", { name: "请求方向" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "响应方向" })).toHaveAttribute("aria-selected", "false");
    expect(screen.getAllByRole("tabpanel")).toHaveLength(1);
    expect(screen.getAllByRole("button", { name: "原样透传" })).toHaveLength(1);
    expect(pluginNames()).toHaveLength(4);
    expect(pluginNames()[0]).toContain("请求压缩");
    expect(pluginNames()[1]).toContain("重试次数 2");
    expect(screen.queryByLabelText("请求压缩 模式")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "配置 响应清理" })).not.toBeInTheDocument();
  });

  it("expands only one plugin's fields at a time", async () => {
    render(<Harness />);
    await userEvent.click(screen.getByRole("button", { name: "配置 请求压缩" }));
    expect(screen.getByLabelText("请求压缩 模式")).toHaveValue("auto");
    expect(screen.getByRole("button", { name: "配置 请求压缩" })).toHaveAttribute("aria-expanded", "true");
    await userEvent.click(screen.getByRole("button", { name: "配置 重试策略" }));
    expect(screen.queryByLabelText("请求压缩 模式")).not.toBeInTheDocument();
    expect(screen.getAllByRole("spinbutton")).toHaveLength(3);
    await userEvent.click(screen.getByRole("button", { name: "配置 重试策略" }));
    expect(screen.queryByRole("spinbutton")).not.toBeInTheDocument();
  });

  it("preserves edited fields and opaque config when switching directions", async () => {
    render(<Harness />);
    await userEvent.click(screen.getByRole("button", { name: "配置 请求压缩" }));
    await userEvent.selectOptions(screen.getByLabelText("请求压缩 模式"), "off");
    await userEvent.click(screen.getByRole("tab", { name: "响应方向" }));
    await userEvent.click(screen.getByRole("button", { name: "配置 响应清理" }));
    await userEvent.selectOptions(screen.getByLabelText("响应清理 模式"), "auto");
    await userEvent.click(screen.getByRole("tab", { name: "请求方向" }));
    await userEvent.click(screen.getByRole("button", { name: "配置 请求压缩" }));
    expect(screen.getByLabelText("请求压缩 模式")).toHaveValue("off");
    expect(current().request.chain[0].config).toEqual({ mode: "off", vendor: { keep: true } });
    expect(current().response.chain[0].config).toEqual({ mode: "auto" });
    expect(current().request.chain[3]).toEqual(fixture().request.chain[3]);
  });

  it("toggles an instance without expanding it or resetting its saved config", async () => {
    render(<Harness />);
    const toggle = screen.getByRole("switch", { name: "启用插件 重试策略" });
    expect(toggle).not.toBeChecked();
    await userEvent.click(toggle);
    expect(toggle).toBeChecked();
    expect(current().request.chain[1]).toEqual({ ...fixture().request.chain[1], enabled: true });
    expect(current().response).toEqual(fixture().response);
    expect(screen.queryByLabelText("重试策略 重试次数")).not.toBeInTheDocument();
  });

  it("re-enables an existing disabled plugin instead of duplicating it", async () => {
    render(<Harness />);
    await userEvent.click(screen.getByRole("switch", { name: "启用插件 重试策略" }));
    await userEvent.click(screen.getByRole("button", { name: "配置 重试策略" }));
    expect(current().request.chain).toHaveLength(4);
    expect(current().request.chain[1]).toEqual({ ...fixture().request.chain[1], enabled: true });
    expect(screen.getByLabelText("重试策略 重试次数")).toHaveValue(2);
  });

  it("adds a new third-party plugin only to the active direction", async () => {
    render(<Harness />);
    await userEvent.click(screen.getByRole("tab", { name: "响应方向" }));
    await userEvent.click(screen.getByRole("button", { name: "添加中间件" }));
    expect(screen.queryByRole("button", { name: "添加 请求压缩" })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "添加 自定义插件" }));
    expect(current().response.chain).toEqual([
      { id: "cleanup", enabled: true, config: { mode: "on" } },
      { id: "pack:custom", enabled: true, config: {} },
    ]);
    expect(current().request).toEqual(fixture().request);
  });

  it("moves the expanded plugin with its details and preserves all configs", async () => {
    render(<Harness />);
    await userEvent.click(screen.getByRole("button", { name: "配置 请求压缩" }));
    expect(screen.getByRole("button", { name: "上移 请求压缩" })).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "下移 请求压缩" }));
    expect(current().request.chain.map((item) => item.id)).toEqual(["retry", "compression", "guard", "missing.plugin"]);
    expect(screen.getByLabelText("请求压缩 模式")).toHaveValue("auto");
    expect(current().request.chain[1]).toEqual(fixture().request.chain[0]);
    expect(pluginNames()[1]).toContain("请求压缩");
    expect(screen.getByRole("button", { name: "配置 请求压缩" })).toHaveFocus();
  });

  it("removes only the chosen row and does not open its neighbor's fields", async () => {
    render(<Harness />);
    await userEvent.click(screen.getByRole("button", { name: "配置 请求压缩" }));
    await userEvent.click(screen.getByRole("button", { name: "移除插件 请求压缩" }));
    expect(current().request.chain).toEqual(fixture().request.chain.slice(1));
    expect(screen.getByRole("button", { name: "配置 重试策略" })).toHaveFocus();
    expect(screen.queryByRole("spinbutton")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "配置 请求压缩" })).not.toBeInTheDocument();
  });

  it("keeps all field kinds editable without erasing unrelated keys", async () => {
    render(<Harness />);
    await userEvent.click(screen.getByRole("button", { name: "配置 重试策略" }));
    await userEvent.clear(screen.getByLabelText("重试策略 重试次数"));
    await userEvent.type(screen.getByLabelText("重试策略 重试次数"), "5");
    expect(current().request.chain[1].config).toEqual({ count: 5, interval: 200, elapsed: 0, custom: [1, 2] });
    await userEvent.click(screen.getByRole("button", { name: "配置 内容检查" }));
    await userEvent.click(screen.getByLabelText("内容检查 严格检查"));
    await userEvent.clear(screen.getByLabelText("内容检查 提示词"));
    await userEvent.type(screen.getByLabelText("内容检查 提示词"), "新的提示词");
    expect(current().request.chain[2].config).toEqual({ strict: true, note: "新的提示词" });
  });

  it("retains a lane's chain when passthrough is selected and reversed", async () => {
    render(<Harness />);
    await userEvent.click(screen.getByRole("button", { name: "原样透传" }));
    expect(screen.queryByRole("list")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("添加插件 - 请求方向")).not.toBeInTheDocument();
    expect(current().request.chain).toEqual(fixture().request.chain);
    await userEvent.click(screen.getByRole("tab", { name: "响应方向" }));
    expect(screen.getByRole("button", { name: "AI Switch 处理" })).toHaveAttribute("aria-pressed", "true");
    await userEvent.click(screen.getByRole("tab", { name: "请求方向" }));
    expect(screen.getByRole("button", { name: "原样透传" })).toHaveAttribute("aria-pressed", "true");
    await userEvent.click(screen.getByRole("button", { name: "AI Switch 处理" }));
    expect(current()).toEqual(fixture());
  });

  it("preserves unavailable plugin data when moving it", async () => {
    render(<Harness />);
    await userEvent.click(screen.getByRole("button", { name: "配置 missing.plugin" }));
    expect(screen.getByRole("button", { name: "下移 missing.plugin" })).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "上移 missing.plugin" }));
    expect(current().request.chain[2]).toEqual(fixture().request.chain[3]);
  });

  it("supports arrow, Home and End navigation in the direction tabs", async () => {
    render(<Harness />);
    const request = screen.getByRole("tab", { name: "请求方向" });
    const response = screen.getByRole("tab", { name: "响应方向" });
    request.focus();
    await userEvent.keyboard("{ArrowRight}");
    expect(response).toHaveFocus();
    expect(response).toHaveAttribute("aria-selected", "true");
    await userEvent.keyboard("{ArrowRight}");
    expect(screen.getByRole("tab", { name: "失败恢复" })).toHaveFocus();
    await userEvent.keyboard("{End}");
    expect(screen.getByRole("tab", { name: "失败恢复" })).toHaveFocus();
    await userEvent.keyboard("{Home}");
    expect(request).toHaveFocus();
  });

  it("shows an empty state and disables Add when no catalog entries exist", () => {
    const initial = fixture();
    initial.request.chain = [];
    render(<Harness initial={initial} entries={[]} />);
    expect(screen.getByText("该方向暂无插件")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "添加中间件" })).toBeDisabled();
  });
});
it("keeps recovery independent and adds catalog defaults through search", async () => {
  const recovery: BuiltinPluginDescriptor = { id:"builtin.recovery.stream", name:"流式恢复", description:"断流后续接已输出的正文", lane:"recovery",kind:"schedule",stage:"policy",defaults:{stream_continue_max:2},config_fields:[{key:"stream_continue_max",label:"续接轮数",kind:"number"}] };
  render(<Harness initial={{version:2,request:{mode:"process",chain:[]},response:{mode:"process",chain:[]},recovery:{mode:"process",chain:[]}}} entries={[...catalog,recovery]} />);
  await userEvent.click(screen.getByRole("tab",{name:"失败恢复"}));
  expect(screen.queryByText(recovery.description)).not.toBeInTheDocument();
  await userEvent.click(screen.getByRole("button",{name:"添加中间件"}));
  await userEvent.type(screen.getByRole("searchbox",{name:"搜索中间件"}),"断流");
  await userEvent.click(screen.getByRole("button",{name:"添加 流式恢复"}));
  expect(current().recovery?.chain).toEqual([{id:recovery.id,enabled:true,config:{stream_continue_max:2}}]);
  expect(current().request.chain).toEqual([]);
});

it("renders the same error-rule and budget editors for an arbitrary external descriptor", async () => {
  const entry: BuiltinPluginDescriptor = {id:"pack:third-party-recovery",name:"外部恢复",description:"custom",lane:"recovery",kind:"schedule",stage:"policy",rule_editor:"retry",class_limits_editor:true,defaults:{rules:[]},config_fields:[]};
  render(<Harness initial={{version:2,request:{mode:"process",chain:[]},response:{mode:"process",chain:[]},recovery:{mode:"process",chain:[{id:entry.id,enabled:true,config:{rules:[]}}]}}} entries={[entry]}/>);
  await userEvent.click(screen.getByRole("button",{name:"配置 外部恢复"}));
  expect(screen.getByText("错误匹配规则")).toBeInTheDocument();
  expect(screen.getByText("按错误类别覆盖重试次数")).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button",{name:"添加错误规则"}));
  expect(current().recovery?.chain[0].config.rules).toEqual([expect.objectContaining({action:"retry",name:"自定义错误"})]);
});

it("lets a descriptor edit request headers without a builtin ID", async()=>{
 const descriptor: BuiltinPluginDescriptor={id:"pack:headers",name:"外部请求头",description:"header rules",lane:"request",kind:"transform",defaults:{user_agent:"",headers:[]},config_fields:[{key:"user_agent",label:"User-Agent",kind:"userAgent"},{key:"headers",label:"请求头",kind:"headers"}]};
 render(<Harness initial={{version:2,catalog_revision:1,request:{mode:"process",chain:[{id:descriptor.id,enabled:true,config:{}}]},response:{mode:"process",chain:[]}}} entries={[descriptor]}/>);
 await userEvent.click(screen.getByRole("button",{name:"配置 外部请求头"}));
 await userEvent.selectOptions(screen.getByLabelText("外部请求头 User-Agent 预设"),"browser");
 await userEvent.click(screen.getByRole("button",{name:"添加请求头"}));
 await userEvent.type(screen.getByLabelText("外部请求头 请求头 1 名称"),"X-Relay");
 await userEvent.selectOptions(screen.getByLabelText("外部请求头 请求头 1 操作"),"set");
 await userEvent.type(screen.getByLabelText("外部请求头 请求头 1 值"),"value");
 expect(current().request.chain[0].config).toMatchObject({user_agent:expect.stringContaining("Mozilla/5.0"),headers:[{name:"X-Relay",value:"value",operation:"set"}]});
 await userEvent.selectOptions(screen.getByLabelText("外部请求头 请求头 1 操作"),"remove");
 expect(screen.queryByLabelText("外部请求头 请求头 1 值")).not.toBeInTheDocument();
});
