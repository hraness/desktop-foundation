import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
const pkg=JSON.parse(await readFile('package.json','utf8'));
const targets=['aarch64-apple-darwin','x86_64-apple-darwin','x86_64-pc-windows-msvc','aarch64-pc-windows-msvc','x86_64-unknown-linux-gnu','aarch64-unknown-linux-gnu'];
const assets=[];
for(const target of targets){
  const name=`hraness-companion-${target}${target.includes('windows')?'.exe':''}`;
  const bytes=await readFile(join('artifacts',name));
  if(!bytes.length || bytes.length>256*1024*1024)throw new Error('Invalid binary size');
  assets.push({target,name,size:bytes.length,sha256:createHash('sha256').update(bytes).digest('hex')});
}
const manifest={schemaVersion:1,version:pkg.version,repository:'hraness/desktop-foundation',tag:`v${pkg.version}`,assets};
if(process.env.GITHUB_REF_TYPE==='tag' && process.env.GITHUB_REF_NAME!==manifest.tag)throw new Error('Package/tag version mismatch');
await writeFile('release-manifest.json',JSON.stringify(manifest,null,2)+'\n');
await writeFile('artifacts/release-manifest.json',JSON.stringify(manifest,null,2)+'\n');
await writeFile('artifacts/SHA256SUMS',assets.map(a=>`${a.sha256}  ${a.name}`).join('\n')+'\n');
