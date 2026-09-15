import { execFileSync } from 'node:child_process';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
const target=process.env.COMPANION_TARGET;
if(!target || !/^[a-z0-9_-]+$/.test(target))throw new Error('Explicit target required');
const binary=resolve(`target/release/hraness-companion${process.platform==='win32'?'.exe':''}`);
function command(program,args){return execFileSync(program,args,{encoding:'utf8',timeout:10_000}).trim()}
const evidence={schemaVersion:1,target,sourceCommit:process.env.GITHUB_SHA,runner:process.env.ImageOS,rust:command('rustc',['--version']),node:process.version,os:process.platform,architecture:process.arch,signing:'no-publisher-signature',notarization:'none',validation:'tests plus native event-loop smoke; visual tray and clean-machine approval are separate'};
if(process.platform==='linux'){
  evidence.distribution=await readFile('/etc/os-release','utf8');
  evidence.libc=command('getconf',['GNU_LIBC_VERSION']);
  evidence.linkedLibraries=command('ldd',[binary]);
} else if(process.platform==='darwin'){
  evidence.minimumOS=command('xcrun',['vtool','-show-build',binary]);
  evidence.linkedLibraries=command('otool',['-L',binary]);
}
await writeFile(`artifacts/hraness-companion-${target}.evidence.json`,JSON.stringify(evidence,null,2)+'\n');
