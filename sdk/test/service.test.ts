import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, rm, chmod, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { companionStatus, stopCompanion, serveCompanion } from '../src/service.js';

test('service owns only authenticated loopback status/stop and cleans up its receipt', async () => {
  const directory = await mkdtemp(join(tmpdir(),'companion-service-'));
  await chmod(directory, 0o700);
  const fake = join(directory, 'native.mjs');
  await writeFile(fake, `import readline from 'node:readline'; const r=readline.createInterface({input:process.stdin}); let ready=false; r.on('line',line=>{const m=JSON.parse(line); if(m.type==='quit'){r.close();process.exit(0)} if(!ready){ready=true;console.log(JSON.stringify({version:1,type:'ready',pid:process.pid,platform:process.platform}))}});r.on('close',()=>process.exit(0));`);
  const appId = 'test.service';
  const running = serveCompanion({ appId, name:'Test',title:'Te', stateDir:directory, binary:process.execPath, binaryArgs:[fake], snapshot:()=>[{kind:'quit',label:'Quit'}], onAction:()=>{} });
  void running.catch(() => {});
  try {
    for (let i=0;i<100 && !(await companionStatus(directory,appId)).running;i++) await delay(20);
    assert.equal((await companionStatus(directory,appId)).running,true);
    const receipt = JSON.parse(await readFile(join(directory,'companion-service.json'),'utf8'));
    const address = `http://127.0.0.1:${receipt.port}`;
    assert.equal((await fetch(address+'/stop',{method:'POST'})).status,403);
    assert.equal((await fetch(address+'/stop',{method:'POST',headers:{Authorization:`Bearer ${receipt.token}`,Origin:'https://untrusted.example'}})).status,403);
    assert.equal((await companionStatus(directory,appId)).running,true);
    assert.equal((await stopCompanion(directory,appId)).running,false);
    assert.equal(await running,0);
    await assert.rejects(readFile(join(directory,'companion-service.json')), {code:'ENOENT'});
  } finally { await stopCompanion(directory,appId); await running; await rm(directory,{recursive:true,force:true}); }
});
