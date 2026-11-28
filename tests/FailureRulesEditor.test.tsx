import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { expect, it, vi } from "vitest";
import { FailureRulesEditor } from "../src/components/FailureRulesEditor";
const { call } = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock("../src/lib/transport", () => ({ getTransport: () => ({ call }) }));
const defaults = { rules: [{ id:"busy",name:"服务繁忙",enabled:true,groups:[[{field:"code",op:"equals",value:"no_capacity"}]],exclude:[],action:"retry",class:"capacity" }] };
function Harness() { const [config,setConfig]=useState<Record<string,unknown>>({});return <><FailureRulesEditor config={config} defaults={defaults} onChange={setConfig}/><output data-testid="config">{JSON.stringify(config)}</output></>; }
it("edits real default conditions and persists explicit rules", async () => {
 render(<Harness/>);
 await userEvent.click(screen.getByText(/服务繁忙 ·/));
 await userEvent.clear(screen.getByLabelText("匹配内容"));
 await userEvent.type(screen.getByLabelText("匹配内容"),"CUSTOM_BUSY");
 const c=JSON.parse(screen.getByTestId("config").textContent!);
 expect(c.rules[0].groups[0][0].value).toBe("CUSTOM_BUSY");
 expect(c.legacy_matching).toBe(false);
 await userEvent.click(screen.getByRole("button",{name:"恢复默认规则"}));
 expect(screen.getByLabelText("匹配内容")).toHaveValue("no_capacity");
});
it("tests the same saved conditions through the native matcher",async()=>{
 call.mockResolvedValue({rule_name:"服务繁忙",action:"retry",observed:{code:"no_capacity"}});
 render(<Harness/>);
 await userEvent.click(screen.getByText("测试错误样例"));
 await userEvent.click(screen.getByRole("button",{name:"测试匹配"}));
 expect(await screen.findByText(/服务繁忙 → 重试/)).toBeInTheDocument();
 expect(call).toHaveBeenCalledWith("builtin_plugin_test_rules",expect.objectContaining({config:expect.objectContaining({rules:defaults.rules}),status:200}));
});
