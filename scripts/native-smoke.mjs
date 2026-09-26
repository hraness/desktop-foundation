import { spawn } from 'node:child_process';
import { mkdtemp, rm, realpath } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
const root = await mkdtemp(join(await realpath(tmpdir()), 'companion-smoke-'));
const binary = resolve(process.argv.slice(2).find(arg => !arg.startsWith('--')) ?? `target/release/hraness-companion${process.platform === 'win32' ? '.exe' : ''}`);
// --v2 drives the same lifecycle with protocol v2 snapshots: a template mark
// with a tone and count, headers, status rows, symbols, subtitles, badges,
// three-state toggles and Option-key alternates.
const v2 = process.argv.includes('--v2');
const initial = v2
  ? { version:2, type:'snapshot', appId:'org.hraness.companion.smoke', name:'Companion Smoke', revision:1, mark:{symbol:'mark.agent',letters:'Te',tone:'attention',text:'3'}, tooltip:'Companion Smoke · 3 waiting', items:[{kind:'header',label:'Companion Smoke'},{kind:'status',symbol:'status.running',label:'Running',detail:'Synthetic smoke test'},{kind:'separator'},{kind:'action',id:'open',label:'Open dashboard',symbol:'action.open',role:'primary',opens:'browser'},{kind:'action',id:'row',label:'Recent row',symbol:'item.file',subtitle:'Two minutes ago',badge:'2',tooltip:'A recent row',alternate:{id:'row.copy',label:'Copy row ID',symbol:'action.copy'}},{kind:'label',label:'Inert row',subtitle:'With detail'},{kind:'action',id:'toggle',label:'Example toggle',state:'mixed'},{kind:'separator'},{kind:'quit',label:'Quit Companion Smoke'}] }
  : { version:1, type:'snapshot', appId:'org.hraness.companion.smoke', name:'Companion Smoke', title:'Te', revision:1, items:[{kind:'label',label:'Synthetic smoke test'}, {kind:'action',id:'toggle',label:'Example toggle',checked:true}, {kind:'quit',label:'Quit'}] };
const toggle = revision => v2 ? {state:['on','off','mixed'][revision%3]} : {checked:revision%2===0};
const nextMark = revision => v2 ? {mark:{...initial.mark,tone:['normal','attention','error','paused','offline'][revision%5],text:revision%5===1||revision%5===2?String(revision):undefined}} : {};
const args = process.argv.includes('--headless') ? ['--check-protocol'] : ['--state-dir',root];
const child = spawn(binary,args,{stdio:['pipe','pipe','pipe']});
const expected = process.argv.includes('--headless') ? 'validated' : 'ready';
let ready = false, quitSent = false, stopped = false, pending = '', errorOutput = '';
let updateTimer;
const timer = setTimeout(()=>child.kill('SIGKILL'),15_000);
try {
  const done = new Promise((resolveDone,reject)=>{
    child.stdin.on('error',reject);
    child.once('error',reject);
    child.stderr.on('data',chunk=>{errorOutput=(errorOutput+chunk).slice(-4000)});
    child.stdout.on('data',chunk=>{
      pending+=chunk;
      let i;
      while((i=pending.indexOf('\n'))>=0){
        const event=JSON.parse(pending.slice(0,i));pending=pending.slice(i+1);
        if(event.type==='error') {reject(new Error(event.code));child.kill();return}
        if(event.type==='stopped') stopped=true;
        if(event.type===expected && !ready){
          ready=true;
          // Keep the real event loop alive through multiple render/refresh
          // cycles. A startup-only probe can miss delayed native failures.
          let revision = 1;
          updateTimer = setInterval(() => {
            if (++revision > 9) {
              clearInterval(updateTimer);
              quitSent = true;
              child.stdin.end(JSON.stringify({version:initial.version,type:'quit'})+'\n');
              return;
            }
            child.stdin.write(JSON.stringify({...initial,...nextMark(revision),revision,items:[{kind:'submenu',label:'Updated '+revision,items:[{kind:'action',id:'toggle',label:'Example toggle',...toggle(revision)}]},...(v2?[{kind:'quit',label:'Quit Companion Smoke'}]:[])]})+'\n');
          },250);
        }
      }
    });
    child.once('close',code=>{if(code!==0 || !ready || !quitSent || (expected==='ready' && !stopped))reject(new Error(`Native smoke failed (${code}; ready=${ready}, quitSent=${quitSent}, stopped=${stopped}): ${errorOutput}`));else resolveDone()});
  });
  child.stdin.write(JSON.stringify(initial)+'\n');
  await done;
  console.log(`${expected}${v2 ? ' (protocol v2)' : ''}: native startup, sustained protocol updates and exit (${process.platform}/${process.arch}); this is not visual tray qualification`);
} finally { clearTimeout(timer); clearInterval(updateTimer); if(child.exitCode===null)child.kill(); await rm(root,{recursive:true,force:true}); }
