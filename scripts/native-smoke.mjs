import { spawn } from 'node:child_process';
import { mkdtemp, rm, realpath } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
const root = await mkdtemp(join(await realpath(tmpdir()), 'companion-smoke-'));
const binary = resolve(process.argv.slice(2).find(arg => !arg.startsWith('--')) ?? `target/release/hraness-companion${process.platform === 'win32' ? '.exe' : ''}`);
const initial = { version:1, type:'snapshot', appId:'org.hraness.companion.smoke', name:'Companion Smoke', title:'Te', revision:1, items:[{kind:'label',label:'Synthetic smoke test'}, {kind:'action',id:'toggle',label:'Example toggle',checked:true}, {kind:'quit',label:'Quit'}] };
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
              child.stdin.end(JSON.stringify({version:1,type:'quit'})+'\n');
              return;
            }
            child.stdin.write(JSON.stringify({...initial,revision,items:[{kind:'submenu',label:'Updated '+revision,items:[{kind:'action',id:'toggle',label:'Example toggle',checked:revision%2===0}]}]})+'\n');
          },250);
        }
      }
    });
    child.once('close',code=>{if(code!==0 || !ready || !quitSent || (expected==='ready' && !stopped))reject(new Error(`Native smoke failed (${code}; ready=${ready}, quitSent=${quitSent}, stopped=${stopped}): ${errorOutput}`));else resolveDone()});
  });
  child.stdin.write(JSON.stringify(initial)+'\n');
  await done;
  console.log(`${expected}: native startup, sustained protocol updates and exit (${process.platform}/${process.arch}); this is not visual tray qualification`);
} finally { clearTimeout(timer); clearInterval(updateTimer); if(child.exitCode===null)child.kill(); await rm(root,{recursive:true,force:true}); }
