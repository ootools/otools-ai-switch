import { describe, expect, it } from "vitest";
import { createDefaultPipeline, resolvePipeline, defaultFailurePolicy, failurePolicyFromPipeline } from "../src/lib/pipeline";

describe("middleware lifecycle", () => {
  it("adds only the two common recovery plugins for a new account", () => {
    const p = createDefaultPipeline();
    expect(p.version).toBe(2);
    expect(p.request.chain).toEqual([]);
    expect(p.response.chain).toEqual([]);
    expect(p.recovery?.chain.map(x => x.id)).toEqual(["builtin.recovery.retry", "builtin.recovery.accountHealth"]);
  });
  it("removed recovery does not silently inherit policy defaults", () => {
    const p = createDefaultPipeline();
    p.recovery = {mode:"process",chain:[]};
    const policy = failurePolicyFromPipeline(p, defaultFailurePolicy());
    expect(policy.retry_count).toBe(0);
    expect(policy.failover_enabled).toBe(false);
    expect(policy.error_status_enabled).toBe(false);
  });
  it("migrates response cleanup without activating a bypassed lane", () => {
    const p = resolvePipeline(JSON.stringify({ pipeline: { version:1, request:{mode:"process",chain:[]}, response:{mode:"passthrough",chain:[{id:"builtin.response.encryptedContentCleanup",enabled:true,config:{aggressive_strip:true}}]} } }));
    expect(p.request.chain).toContainEqual({id:"builtin.request.encryptedContentCleanup",enabled:false,config:{aggressive_strip:true}});
    expect(p.response.chain).toEqual([]);
    expect(resolvePipeline(JSON.stringify({pipeline:p}))).toEqual(p);
  });
  it("preserves old runtime retry defaults, not the misleading disabled row", () => {
    const p = resolvePipeline(JSON.stringify({pipeline:{version:1,request:{mode:"process",chain:[{id:"builtin.schedule.retry",enabled:false,config:{retry_count:8}}]},response:{mode:"process",chain:[]}}}));
    expect(failurePolicyFromPipeline(p, defaultFailurePolicy()).retry_count).toBe(2);
    expect(p.recovery?.chain[0].config.retry_count).toBe(2);
  });
});

it("migrates account headers and combines image options once",()=>{
 const config={headers:{"User-Agent":"OldBot/1","X-Relay":"value"},pipeline:{version:2,request:{mode:"process",chain:[{id:"builtin.request.inlineRemoteImages",enabled:true,config:{vendor:"keep"}},{id:"builtin.request.imageLimit",enabled:false,config:{}}]},response:{mode:"process",chain:[]},recovery:{mode:"process",chain:[]}}};
 const p=resolvePipeline(JSON.stringify(config));
 expect(p.request.chain.map(x=>x.id)).toEqual(["builtin.request.imageProcessing","builtin.request.requestHeaders"]);
 expect(p.request.chain[0].config).toMatchObject({inline_remote:true,trim_old_images:false,max_images:10});
 expect(p.request.chain[1].config).toMatchObject({user_agent:"OldBot/1",headers:[{name:"X-Relay",value:"value",operation:"fill"}]});
 expect(resolvePipeline(JSON.stringify({...config,pipeline:p}))).toEqual(p);
 p.request.chain=[];
 expect(resolvePipeline(JSON.stringify({...config,pipeline:p})).request.chain).toEqual([]);
});

it("preserves old headers under passthrough without activating the previous request chain",()=>{
 const p=resolvePipeline(JSON.stringify({headers:{"User-Agent":"Pinned/1"},pipeline:{version:2,request:{mode:"passthrough",chain:[{id:"builtin.request.compression",enabled:true,config:{mode:"on"}}]},response:{mode:"process",chain:[]}}}));
 expect(p.request.mode).toBe("process");
 expect(p.request.chain.find(x=>x.id==="builtin.request.compression")?.enabled).toBe(false);
 expect(p.request.chain.find(x=>x.id==="builtin.request.requestHeaders")?.enabled).toBe(true);
});
it("archives redundant old image entries when a merged plugin is already present",()=>{
 const p=resolvePipeline(JSON.stringify({pipeline:{version:2,request:{mode:"process",chain:[{id:"builtin.request.inlineRemoteImages",enabled:true,config:{vendor:"keep"}},{id:"builtin.request.imageProcessing",enabled:false,config:{inline_remote:false,trim_old_images:false}}]},response:{mode:"process",chain:[]}}}));
 expect(p.request.chain.map(x=>x.id)).toEqual(["builtin.request.imageProcessing"]);
 expect(p.request.chain[0].enabled).toBe(false);
 expect(p.request.chain[0].config.legacy_plugins).toEqual([expect.objectContaining({id:"builtin.request.inlineRemoteImages",config:{vendor:"keep"}})]);
});

it("normalizes omitted v2 lanes without reviving stale flat settings",()=>{
 const p=resolvePipeline(JSON.stringify({headers:{"User-Agent":"Stale/1"},inline_remote_images:true,pipeline:{version:2,catalog_revision:1,request:{mode:"process",chain:[]}}}));
 expect(p.request.chain).toEqual([]);
 expect(p.response).toEqual({mode:"process",chain:[]});
});
