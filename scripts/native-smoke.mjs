import { spawn } from 'node:child_process';
import { mkdtemp, rm, realpath } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
const root = await mkdtemp(join(await realpath(tmpdir()), 'companion-smoke-'));
const binary = resolve(process.argv.slice(2).find(arg => !arg.startsWith('--')) ?? `target/release/hraness-companion${process.platform === 'win32' ? '.exe' : ''}`);
const initial = { version:1, type:'snapshot', appId:'org.hraness.companion.smoke', name:'Companion Smoke', title:'Te', revision:1, items:[{kind:'label',label:'Synthetic smoke test'}, {kind:'action',id:'toggle',label:'Example toggle',checked:true}, {kind:'quit',label:'Quit'}] };
const args = ['--state-dir',root, ...(process.argv.includes('--headless') ? ['--check-protocol'] : [])];
const child = spawn(binary,args,{stdio:['pipe','pipe','pipe']});
const expected = process.argv.includes('--headless') ? 'validated' : 'ready';
let ready = false, pending = '', errorOutput = '';
const timer = setTimeout(()=>child.kill('SIGKILL'),15_000);
try {
  const done = new Promise((resolveDone,reject)=>{
    child.once('error',reject);
    child.stderr.on('data',chunk=>{errorOutput=(errorOutput+chunk).slice(-4000)});
    child.stdout.on('data',chunk=>{
      pending+=chunk;
      let i;
      while((i=pending.indexOf('\n'))>=0){
        const event=JSON.parse(pending.slice(0,i));pending=pending.slice(i+1);
        if(event.type==='error') {reject(new Error(event.code));child.kill();return}
        if(event.type===expected && !ready){
          ready=true;
          child.stdin.write(JSON.stringify({...initial,revision:2,items:[{kind:'submenu',label:'Updated',items:[{kind:'action',id:'toggle',label:'Example toggle',checked:false}]}]})+'\n');
          setTimeout(()=>child.stdin.end(JSON.stringify({version:1,type:'quit'})+'\n'),250);
        }
      }
    });
    child.once('close',code=>{if(code!==0 || !ready)reject(new Error(`Native smoke failed (${code}): ${errorOutput}`));else resolveDone()});
  });
  child.stdin.write(JSON.stringify(initial)+'\n');
  await done;
  console.log(`${expected}: native startup, protocol update and exit (${process.platform}/${process.arch}); this is not visual tray qualification`);
} finally { clearTimeout(timer); if(child.exitCode===null)child.kill(); await rm(root,{recursive:true,force:true}); }
