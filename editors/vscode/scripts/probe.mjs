// Dump what VHDL-LS answers for a project: the hover text on every type and port declared in
// `fsm.vhd`, which is how the fixtures in `src/generate.test.ts` were produced.
//
//   VHDL_LS=/path/to/vhdl_ls node scripts/probe.mjs <directory holding fsm.vhd>
//
// Every generator reads the server's own answers rather than parsing VHDL, so when a generator
// misreads something, this shows what the server actually said. It needs a `vhdl_ls` that can
// find its `vhdl_libraries`; a build installed with cargo alone panics without them.
import { spawn } from "node:child_process";
import { readFileSync, readdirSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { join } from "node:path";
const root=process.argv[2];
const ls=spawn(process.env.VHDL_LS??"vhdl_ls",[],{cwd:root,stdio:["pipe","pipe","inherit"]});
let id=0,buf=Buffer.alloc(0);const pending=new Map();
ls.stdout.on("data",d=>{buf=Buffer.concat([buf,d]);for(;;){const h=buf.indexOf("\r\n\r\n");if(h<0)return;
const len=+/Content-Length: (\d+)/.exec(buf.slice(0,h).toString())[1];if(buf.length<h+4+len)return;
const m=JSON.parse(buf.slice(h+4,h+4+len).toString());buf=buf.slice(h+4+len);
if(pending.has(m.id)){pending.get(m.id)(m.result);pending.delete(m.id);}}});
const send=m=>{const s=JSON.stringify(m);ls.stdin.write(`Content-Length: ${Buffer.byteLength(s)}\r\n\r\n${s}`);};
const req=(me,p)=>new Promise(r=>{const i=++id;pending.set(i,r);send({jsonrpc:"2.0",id:i,method:me,params:p});});
const note=(me,p)=>send({jsonrpc:"2.0",method:me,params:p});
await req("initialize",{processId:process.pid,rootUri:pathToFileURL(root).href,
 capabilities:{textDocument:{documentSymbol:{hierarchicalDocumentSymbolSupport:true},hover:{}},workspace:{symbol:{}}}});
note("initialized",{});
for(const f of readdirSync(root).filter(f=>f.endsWith(".vhd")))
 note("textDocument/didOpen",{textDocument:{uri:pathToFileURL(join(root,f)).href,languageId:"vhdl",version:1,text:readFileSync(join(root,f),"utf8")}});
await new Promise(r=>setTimeout(r,2000));
const uri=pathToFileURL(join(root,"fsm.vhd")).href;
const syms=await req("textDocument/documentSymbol",{textDocument:{uri}});
const flat=[];const walk=s=>{flat.push(s);(s.children??[]).forEach(walk);};(syms??[]).forEach(walk);
for(const s of flat.filter(s=>/type|port/i.test(s.name))){
  const h=await req("textDocument/hover",{textDocument:{uri},position:s.selectionRange.start});
  console.log(`\n[${s.kind}] ${s.name}\n  hover: ${JSON.stringify(h?.contents?.value ?? h?.contents)}`);
}
ls.kill();
