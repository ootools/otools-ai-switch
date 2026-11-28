import { USER_AGENT_PRESETS, matchUserAgentPreset } from "../lib/accountUserAgent";

type HeaderRule = { name: string; value: string; operation: "fill" | "set" | "remove"; enabled?: boolean };
export function UserAgentField({ label, value, onChange }: { label: string; value: unknown; onChange: (v: unknown) => void }) {
  const text=typeof value === "string" ? value : "";
  return <div className="pipeline-field pipeline-field-wide"><label>{label} 预设<select aria-label={`${label} 预设`} value={matchUserAgentPreset(text)} onChange={e=>{const item=USER_AGENT_PRESETS.find(p=>p.id===e.target.value);if(item && item.id!=="custom")onChange(item.value);}}>{USER_AGENT_PRESETS.map(p=><option key={p.id} value={p.id}>{p.label}</option>)}</select></label><label>{label}<input aria-label={label} value={text} placeholder="留空保留默认客户端身份" onChange={e=>onChange(e.target.value)}/></label></div>;
}
export function HeaderRulesField({ label, value, onChange }: { label: string; value: unknown; onChange: (v: unknown) => void }) {
  const rows=(Array.isArray(value) ? value : []) as HeaderRule[];
  const update=(index:number,patch:Partial<HeaderRule>)=>onChange(rows.map((r,i)=>i===index?{...r,...patch}:r));
  return <fieldset className="pipeline-header-rules pipeline-field-wide"><legend>{label}</legend><p>补充：仅头不存在时写入；覆盖：替换已有值；移除：发送前删除。</p>
    {rows.map((row,i)=><div className="pipeline-header-rule" key={i}>
      <input aria-label={`${label} ${i+1} 名称`} placeholder="X-Custom-Header" value={row.name} onChange={e=>update(i,{name:e.target.value})}/>
      <select aria-label={`${label} ${i+1} 操作`} value={row.operation||"fill"} onChange={e=>update(i,{operation:e.target.value as HeaderRule["operation"]})}><option value="fill">补充</option><option value="set">覆盖</option><option value="remove">移除</option></select>
      {row.operation!=="remove" ? <input aria-label={`${label} ${i+1} 值`} placeholder="请求头值" value={row.value} onChange={e=>update(i,{value:e.target.value})}/> : null}
      <button type="button" aria-label={`${label} 删除第 ${i+1} 行`} onClick={()=>onChange(rows.filter((_,n)=>n!==i))}>删除</button>
    </div>)}
    <button type="button" onClick={()=>onChange([...rows,{name:"",value:"",operation:"fill"}])}>添加请求头</button>
  </fieldset>;
}
