// Terminal column widths, measured the way the Rust kit's ratatui buffer
// measures them: text is split into grapheme clusters, each cluster is as
// wide as `unicode-width` 0.2 says its string is (plus one column for each
// halfwidth katakana sound mark, as ratatui adds), and zero-width clusters
// take no cell. The tables below are generated from unicode-width 0.2.2
// (Unicode 17.0.0), the version Cargo.lock pins, by
// scripts/gen-text-width.sh. The per-cluster rules port the emoji,
// variation-selector and regional-indicator parts of unicode-width's
// `width_in_str`; the script ligature rules (Arabic lam-alef and a few
// others) are not ported, so those clusters use the sum of their base widths.

// `start[-end]:width` in hex, for every code point from U+00A0 whose width is not 1.
const WIDTH =
  'ad:0,300-36f:0,483-489:0,591-5bd:0,5bf:0,5c1-5c2:0,5c4-5c5:0,5c7:0,605:0,610-61a:0,61c:0,64b-65f:0,670:0,6d6-6' +
  'dc:0,6df-6e4:0,6e7-6e8:0,6ea-6ed:0,70f:0,711:0,730-74a:0,7a6-7b0:0,7eb-7f3:0,7fd:0,816-819:0,81b-823:0,825-827' +
  ':0,829-82d:0,859-85b:0,890-891:0,897-89f:0,8ca-902:0,93a:0,93c:0,941-948:0,94d:0,951-957:0,962-963:0,981:0,9bc' +
  ':0,9be:0,9c1-9c4:0,9cd:0,9d7:0,9e2-9e3:0,9fe:0,a01-a02:0,a3c:0,a41-a42:0,a47-a48:0,a4b-a4d:0,a51:0,a70-a71:0,a' +
  '75:0,a81-a82:0,abc:0,ac1-ac5:0,ac7-ac8:0,acd:0,ae2-ae3:0,afa-aff:0,b01:0,b3c:0,b3e-b3f:0,b41-b44:0,b4d:0,b55-b' +
  '57:0,b62-b63:0,b82:0,bbe:0,bc0:0,bcd:0,bd7:0,c00:0,c04:0,c3c:0,c3e-c40:0,c46-c48:0,c4a-c4d:0,c55-c56:0,c62-c63' +
  ':0,c81:0,cbc:0,cbf-cc0:0,cc2:0,cc6-cc8:0,cca-ccd:0,cd5-cd6:0,ce2-ce3:0,d00-d01:0,d3b-d3c:0,d3e:0,d41-d44:0,d4d' +
  '-d4e:0,d57:0,d62-d63:0,d81:0,dca:0,dcf:0,dd2-dd4:0,dd6:0,ddf:0,e31:0,e34-e3a:0,e47-e4e:0,eb1:0,eb4-ebc:0,ec8-e' +
  'ce:0,f18-f19:0,f35:0,f37:0,f39:0,f71-f7e:0,f80-f84:0,f86-f87:0,f8d-f97:0,f99-fbc:0,fc6:0,102d-1030:0,1032-1037' +
  ':0,1039-103a:0,103d-103e:0,1058-1059:0,105e-1060:0,1071-1074:0,1082:0,1085-1086:0,108d:0,109d:0,1100-115f:2,11' +
  '60-11ff:0,135d-135f:0,1712-1715:0,1732-1734:0,1752-1753:0,1772-1773:0,17a4:2,17b4-17b5:0,17b7-17bd:0,17c6:0,17' +
  'c9-17d3:0,17d8:3,17dd:0,180b-180f:0,1885-1886:0,18a9:0,1920-1922:0,1927-1928:0,1932:0,1939-193b:0,1a17-1a18:0,' +
  '1a1b:0,1a56:0,1a58-1a5e:0,1a60:0,1a62:0,1a65-1a6c:0,1a73-1a7c:0,1a7f:0,1ab0-1add:0,1ae0-1aeb:0,1b00-1b03:0,1b3' +
  '4-1b3d:0,1b42-1b44:0,1b6b-1b73:0,1b80-1b81:0,1ba2-1ba5:0,1ba8-1bad:0,1be6:0,1be8-1be9:0,1bed:0,1bef-1bf3:0,1c2' +
  'c-1c33:0,1c36-1c37:0,1cd0-1cd2:0,1cd4-1ce0:0,1ce2-1ce8:0,1ced:0,1cf4:0,1cf8-1cf9:0,1dc0-1dff:0,200b-200f:0,202' +
  'a-202e:0,2060-206f:0,20d0-20f0:0,231a-231b:2,2329-232a:2,23e9-23ec:2,23f0:2,23f3:2,25fd-25fe:2,2614-2615:2,263' +
  '0-2637:2,2648-2653:2,267f:2,268a-268f:2,2693:2,26a1:2,26aa-26ab:2,26bd-26be:2,26c4-26c5:2,26ce:2,26d4:2,26ea:2' +
  ',26f2-26f3:2,26f5:2,26fa:2,26fd:2,2705:2,270a-270b:2,2728:2,274c:2,274e:2,2753-2755:2,2757:2,2795-2797:2,27b0:' +
  '2,27bf:2,2b1b-2b1c:2,2b50:2,2b55:2,2cef-2cf1:0,2de0-2dff:0,2e80-2e99:2,2e9b-2ef3:2,2f00-2fd5:2,2ff0-3029:2,302' +
  'a-302f:0,3030-303e:2,3041-3096:2,3099-309a:0,309b-30ff:2,3105-312f:2,3131-3163:2,3164:0,3165-318e:2,3190-31e5:' +
  '2,31ef-321e:2,3220-3247:2,3250-a48c:2,a490-a4c6:2,a66f-a672:0,a674-a67d:0,a69e-a69f:0,a6f0-a6f1:0,a802:0,a806:' +
  '0,a80b:0,a825-a826:0,a82c:0,a8c4-a8c5:0,a8e0-a8f1:0,a8fa:0,a8ff:0,a926-a92d:0,a947-a951:0,a953:0,a960-a97c:2,a' +
  '980-a982:0,a9b3:0,a9b6-a9b9:0,a9bc-a9bd:0,a9c0:0,a9e5:0,aa29-aa2e:0,aa31-aa32:0,aa35-aa36:0,aa43:0,aa4c:0,aa7c' +
  ':0,aab0:0,aab2-aab4:0,aab7-aab8:0,aabe-aabf:0,aac1:0,aaec-aaed:0,aaf6:0,abe5:0,abe8:0,abed:0,ac00-d7a3:2,d7b0-' +
  'd7c6:0,d7cb-d7fb:0,f900-faff:2,fb1e:0,fe00-fe0f:0,fe10-fe19:2,fe20-fe2f:0,fe30-fe52:2,fe54-fe66:2,fe68-fe6b:2,' +
  'feff:0,ff01-ff60:2,ff9e-ffa0:0,ffe0-ffe6:2,fff0-fff8:0,101fd:0,102e0:0,10376-1037a:0,10a01-10a03:0,10a05-10a06' +
  ':0,10a0c-10a0f:0,10a38-10a3a:0,10a3f:0,10ae5-10ae6:0,10d24-10d27:0,10d69-10d6d:0,10eab-10eac:0,10efa-10eff:0,1' +
  '0f46-10f50:0,10f82-10f85:0,11001:0,11038-11046:0,11070:0,11073-11074:0,1107f-11081:0,110b3-110b6:0,110b9-110ba' +
  ':0,110c2:0,11100-11102:0,11127-1112b:0,1112d-11134:0,11173:0,11180-11181:0,111b6-111be:0,111c0:0,111c2-111c3:0' +
  ',111c9-111cc:0,111cf:0,1122f-11231:0,11234-11237:0,1123e:0,11241:0,112df:0,112e3-112ea:0,11300-11301:0,1133b-1' +
  '133c:0,1133e:0,11340:0,1134d:0,11357:0,11366-1136c:0,11370-11374:0,113b8:0,113bb-113c0:0,113c2:0,113c5:0,113c7' +
  '-113c9:0,113ce-113d2:0,113e1-113e2:0,11438-1143f:0,11442-11444:0,11446:0,1145e:0,114b0:0,114b3-114b8:0,114ba:0' +
  ',114bd:0,114bf-114c0:0,114c2-114c3:0,115af:0,115b2-115b5:0,115bc-115bd:0,115bf-115c0:0,115dc-115dd:0,11633-116' +
  '3a:0,1163d:0,1163f-11640:0,116ab:0,116ad:0,116b0-116b7:0,1171d:0,1171f:0,11722-11725:0,11727-1172b:0,1182f-118' +
  '37:0,11839-1183a:0,11930:0,1193b-1193f:0,11941:0,11943:0,119d4-119d7:0,119da-119db:0,119e0:0,11a01-11a0a:0,11a' +
  '33-11a38:0,11a3b-11a3e:0,11a47:0,11a51-11a56:0,11a59-11a5b:0,11a84-11a96:0,11a98-11a99:0,11b60:0,11b62-11b64:0' +
  ',11b66:0,11c30-11c36:0,11c38-11c3d:0,11c3f:0,11c92-11ca7:0,11caa-11cb0:0,11cb2-11cb3:0,11cb5-11cb6:0,11d31-11d' +
  '36:0,11d3a:0,11d3c-11d3d:0,11d3f-11d47:0,11d90-11d91:0,11d95:0,11d97:0,11ef3-11ef4:0,11f00-11f02:0,11f36-11f3a' +
  ':0,11f40-11f42:0,11f5a:0,13440:0,13447-13455:0,1611e-16129:0,1612d-1612f:0,16af0-16af4:0,16b30-16b36:0,16f4f:0' +
  ',16f8f-16f92:0,16fe0-16fe3:2,16fe4:0,16ff0-16ff1:0,16ff2-16ff6:2,17000-18cd5:2,18cff-18d1e:2,18d80-18df2:2,1af' +
  'f0-1aff3:2,1aff5-1affb:2,1affd-1affe:2,1b000-1b122:2,1b132:2,1b150-1b152:2,1b155:2,1b164-1b167:2,1b170-1b2fb:2' +
  ',1bc9d-1bc9e:0,1bca0-1bca3:0,1cf00-1cf2d:0,1cf30-1cf46:0,1d165-1d169:0,1d16d-1d182:0,1d185-1d18b:0,1d1aa-1d1ad' +
  ':0,1d242-1d244:0,1d300-1d356:2,1d360-1d376:2,1da00-1da36:0,1da3b-1da6c:0,1da75:0,1da84:0,1da9b-1da9f:0,1daa1-1' +
  'daaf:0,1e000-1e006:0,1e008-1e018:0,1e01b-1e021:0,1e023-1e024:0,1e026-1e02a:0,1e08f:0,1e130-1e136:0,1e2ae:0,1e2' +
  'ec-1e2ef:0,1e4ec-1e4ef:0,1e5ee-1e5ef:0,1e6e3:0,1e6e6:0,1e6ee-1e6ef:0,1e6f5:0,1e8d0-1e8d6:0,1e944-1e94a:0,1f004' +
  ':2,1f0cf:2,1f18e:2,1f191-1f19a:2,1f200-1f202:2,1f210-1f23b:2,1f240-1f248:2,1f250-1f251:2,1f260-1f265:2,1f300-1' +
  'f320:2,1f32d-1f335:2,1f337-1f37c:2,1f37e-1f393:2,1f3a0-1f3ca:2,1f3cf-1f3d3:2,1f3e0-1f3f0:2,1f3f4:2,1f3f8-1f43e' +
  ':2,1f440:2,1f442-1f4fc:2,1f4ff-1f53d:2,1f54b-1f54e:2,1f550-1f567:2,1f57a:2,1f595-1f596:2,1f5a4:2,1f5fb-1f64f:2' +
  ',1f680-1f6c5:2,1f6cc:2,1f6d0-1f6d2:2,1f6d5-1f6d8:2,1f6dc-1f6df:2,1f6eb-1f6ec:2,1f6f4-1f6fc:2,1f7e0-1f7eb:2,1f7' +
  'f0:2,1f90c-1f93a:2,1f93c-1f945:2,1f947-1f9ff:2,1fa70-1fa7c:2,1fa80-1fa8a:2,1fa8e-1fac6:2,1fac8:2,1facd-1fadc:2' +
  ',1fadf-1faea:2,1faef-1faf8:2,20000-2fffd:2,30000-3fffd:2,e0000-e0fff:0';
// Code points that form an emoji presentation sequence with U+FE0F.
const VS16 =
  '23,2a,30-39,a9,ae,203c,2049,2122,2139,2194-2199,21a9-21aa,231a-231b,2328,23cf,23e9-23f3,23f8-23fa,24c2,25aa-25' +
  'ab,25b6,25c0,25fb-25fe,2600-2604,260e,2611,2614-2615,2618,261d,2620,2622-2623,2626,262a,262e-262f,2638-263a,26' +
  '40,2642,2648-2653,265f-2660,2663,2665-2666,2668,267b,267e-267f,2692-2697,2699,269b-269c,26a0-26a1,26a7,26aa-26' +
  'ab,26b0-26b1,26bd-26be,26c4-26c5,26c8,26ce-26cf,26d1,26d3-26d4,26e9-26ea,26f0-26f5,26f7-26fa,26fd,2702,2705,27' +
  '08-270d,270f,2712,2714,2716,271d,2721,2728,2733-2734,2744,2747,274c,274e,2753-2755,2757,2763-2764,2795-2797,27' +
  'a1,27b0,27bf,2934-2935,2b05-2b07,2b1b-2b1c,2b50,2b55,3030,303d,3297,3299,1f004,1f170-1f171,1f17e-1f17f,1f202,1' +
  'f21a,1f22f,1f237,1f30d-1f30f,1f315,1f31c,1f321,1f324-1f32c,1f336,1f378,1f37d,1f393,1f396-1f397,1f399-1f39b,1f3' +
  '9e-1f39f,1f3a7,1f3ac-1f3ae,1f3c2,1f3c4,1f3c6,1f3ca-1f3ce,1f3d4-1f3e0,1f3ed,1f3f3,1f3f5,1f3f7,1f408,1f415,1f41f' +
  ',1f426,1f43f,1f441-1f442,1f446-1f449,1f44d-1f44e,1f453,1f46a,1f47d,1f4a3,1f4b0,1f4b3,1f4bb,1f4bf,1f4cb,1f4da,1' +
  'f4df,1f4e4-1f4e6,1f4ea-1f4ed,1f4f7,1f4f9-1f4fb,1f4fd,1f508,1f50d,1f512-1f513,1f549-1f54a,1f550-1f567,1f56f-1f5' +
  '70,1f573-1f579,1f587,1f58a-1f58d,1f590,1f5a5,1f5a8,1f5b1-1f5b2,1f5bc,1f5c2-1f5c4,1f5d1-1f5d3,1f5dc-1f5de,1f5e1' +
  ',1f5e3,1f5e8,1f5ef,1f5f3,1f5fa,1f610,1f687,1f68d,1f691,1f694,1f698,1f6ad,1f6b2,1f6b9-1f6ba,1f6bc,1f6cb,1f6cd-1' +
  'f6cf,1f6e0-1f6e5,1f6e9,1f6f0,1f6f3';
// Code points with a non-ideographic text presentation sequence with U+FE0E.
const VS15 =
  '231a-231b,23e9-23ec,23f0,23f3,25fd-25fe,2614-2615,2648-2653,267f,2693,26a1,26aa-26ab,26bd-26be,26c4-26c5,26ce,' +
  '26d4,26ea,26f2-26f3,26f5,26fa,26fd,2705,270a-270b,2728,274c,274e,2753-2755,2757,2795-2797,27b0,27bf,2b1b-2b1c,' +
  '2b50,2b55,1f004,1f30d-1f30f,1f315,1f31c,1f378,1f393,1f3a7,1f3ac-1f3ae,1f3c2,1f3c4,1f3c6,1f3ca,1f3e0,1f3ed,1f40' +
  '8,1f415,1f41f,1f426,1f442,1f446-1f449,1f44d-1f44e,1f453,1f46a,1f47d,1f4a3,1f4b0,1f4b3,1f4bb,1f4bf,1f4cb,1f4da,' +
  '1f4df,1f4e4-1f4e6,1f4ea-1f4ed,1f4f7,1f4f9-1f4fb,1f508,1f50d,1f512-1f513,1f550-1f567,1f610,1f687,1f68d,1f691,1f' +
  '694,1f698,1f6ad,1f6b2,1f6b9-1f6ba,1f6bc';
// Emoji_Modifier_Base.
const MODBASE =
  '261d,26f9,270a-270d,1f385,1f3c2-1f3c4,1f3c7,1f3ca-1f3cc,1f442-1f443,1f446-1f450,1f466-1f478,1f47c,1f481-1f483,' +
  '1f485-1f487,1f48f,1f491,1f4aa,1f574-1f575,1f57a,1f590,1f595-1f596,1f645-1f647,1f64b-1f64f,1f6a3,1f6b4-1f6b6,1f' +
  '6c0,1f6cc,1f90c,1f90f,1f918-1f91f,1f926,1f930-1f939,1f93c-1f93e,1f977,1f9b5-1f9b6,1f9b8-1f9b9,1f9bb,1f9cd-1f9c' +
  'f,1f9d1-1f9dd,1fac3-1fac5,1faf0-1faf8';
// Code points unicode-width tags as emoji presentation (width 2 that joins ZWJ sequences).
const EP =
  '231a-231b,23e9-23ec,23f0,23f3,25fd-25fe,2614-2615,2648-2653,267f,2693,26a1,26aa-26ab,26bd-26be,26c4-26c5,26ce,' +
  '26d4,26ea,26f2-26f3,26f5,26fa,26fd,2705,270a-270b,2728,274c,274e,2753-2755,2757,2795-2797,27b0,27bf,2b1b-2b1c,' +
  '2b50,2b55,1f004,1f0cf,1f18e,1f191-1f19a,1f201,1f21a,1f22f,1f232-1f236,1f238-1f23a,1f250-1f251,1f300-1f320,1f32' +
  'd-1f335,1f337-1f37c,1f37e-1f393,1f3a0-1f3ca,1f3cf-1f3d3,1f3e0-1f3f0,1f3f4,1f3f8-1f3fa,1f400-1f43e,1f440,1f442-' +
  '1f4fc,1f4ff-1f53d,1f54b-1f54e,1f550-1f567,1f57a,1f595-1f596,1f5a4,1f5fb-1f64f,1f680-1f6c5,1f6cc,1f6d0-1f6d2,1f' +
  '6d5-1f6d8,1f6dc-1f6df,1f6eb-1f6ec,1f6f4-1f6fc,1f7e0-1f7eb,1f7f0,1f90c-1f93a,1f93c-1f945,1f947-1f9ff,1fa70-1fa7' +
  'c,1fa80-1fa8a,1fa8e-1fac6,1fac8,1facd-1fadc,1fadf-1faea,1faef-1faf8';

type Ranges = { lo: Uint32Array; hi: Uint32Array; value: Uint8Array };
function parse(spec: string, withValue: boolean): Ranges {
  const parts = spec.split(',');
  const lo = new Uint32Array(parts.length), hi = new Uint32Array(parts.length), value = new Uint8Array(parts.length);
  parts.forEach((part, i) => {
    const [range, w] = withValue ? part.split(':') : [part, '1'];
    const [a, b] = range.split('-');
    lo[i] = parseInt(a, 16); hi[i] = parseInt(b ?? a, 16); value[i] = Number(w);
  });
  return { lo, hi, value };
}
function find(r: Ranges, cp: number): number {
  let a = 0, b = r.lo.length - 1;
  while (a <= b) {
    const m = (a + b) >> 1;
    if (cp < r.lo[m]) b = m - 1; else if (cp > r.hi[m]) a = m + 1; else return m;
  }
  return -1;
}
let tables: { width: Ranges; vs16: Ranges; vs15: Ranges; modBase: Ranges; ep: Ranges } | undefined;
function t() {
  return tables ??= { width: parse(WIDTH, true), vs16: parse(VS16, false), vs15: parse(VS15, false), modBase: parse(MODBASE, false), ep: parse(EP, false) };
}

/** The width unicode-width gives one code point on its own (control characters count 1 here; callers clean them first). */
export function codePointWidth(cp: number): number {
  if (cp < 0xa0) return 1;
  const i = find(t().width, cp);
  return i < 0 ? 1 : t().width.value[i];
}

const enum Info {
  Default, Modifier, RegionalIndicator, SeveralRI, EmojiPresentation, ZwjEp, Vs16ZwjEp, KeycapZwjEp, Vs16KeycapZwjEp,
  RiZwj, EvenRiZwj, OddRiZwj, TagEnd, TagD1, TagD2, TagD3, TagA1, TagA2, TagA3, TagA4, TagA5, TagA6, Vs15, Vs16, Vs123,
}
const isRI = (cp: number) => cp >= 0x1f1e6 && cp <= 0x1f1ff;
const isTagLetter = (cp: number) => cp >= 0xe0061 && cp <= 0xe007a;
const isTagDigit = (cp: number) => cp >= 0xe0030 && cp <= 0xe0039;
function lookup(cp: number): [number, Info] {
  if (cp === 0xfe0e) return [0, Info.Vs15];
  if (cp === 0xfe0f) return [0, Info.Vs16];
  if (cp === 0xfe01) return [0, Info.Vs123];
  if (isRI(cp)) return [1, Info.RegionalIndicator];
  if (cp >= 0x1f3fb && cp <= 0x1f3ff) return [2, Info.Modifier];
  if (find(t().ep, cp) >= 0) return [2, Info.EmojiPresentation];
  return [codePointWidth(cp), Info.Default];
}
function step(cp: number, next: Info): [number, Info] {
  if (next === Info.Vs16 || next === Info.Vs16ZwjEp || next === Info.Vs16KeycapZwjEp) {
    if (find(t().vs16, cp) >= 0) return [next === Info.Vs16 ? 2 : 0, Info.EmojiPresentation];
    next = Info.Default;
  }
  if (cp <= 0xa0) return [1, Info.Default];
  if (next !== Info.Default) {
    if (cp === 0xfe0f) return [0, next === Info.ZwjEp ? Info.Vs16ZwjEp : next === Info.KeycapZwjEp ? Info.Vs16KeycapZwjEp : Info.Vs16];
    if (cp === 0xfe01) return [0, Info.Vs123];
    if (cp === 0xfe0e) return [0, Info.Vs15];
    if (next === Info.Vs15) {
      if (find(t().vs15, cp) >= 0) return [1, Info.Default];
      next = Info.Default;
    } else if (next === Info.Vs123) {
      if (cp === 0x2018 || cp === 0x2019 || cp === 0x201c || cp === 0x201d) return [2, Info.Default];
      next = Info.Default;
    }
    switch (next) {
      case Info.Modifier: if (find(t().modBase, cp) >= 0) return [0, Info.EmojiPresentation]; break;
      case Info.RegionalIndicator: case Info.SeveralRI: if (isRI(cp)) return [1, Info.SeveralRI]; break;
      case Info.RiZwj: case Info.OddRiZwj: if (isRI(cp)) return [-1, Info.EvenRiZwj]; break;
      case Info.EvenRiZwj: if (isRI(cp)) return [3, Info.OddRiZwj]; break;
      default: break;
    }
    if (cp === 0x200d && (next === Info.EmojiPresentation || next === Info.SeveralRI || next === Info.EvenRiZwj || next === Info.OddRiZwj || next === Info.Modifier)) return [0, Info.ZwjEp];
    if (next === Info.ZwjEp) {
      if (cp === 0x20e3) return [0, Info.KeycapZwjEp];
      if (isRI(cp)) return [1, Info.RiZwj];
      if (cp >= 0x1f3fb && cp <= 0x1f3ff) return [0, Info.Modifier];
      if (cp === 0xe007f) return [0, Info.TagEnd];
    }
    if (isTagLetter(cp)) {
      const after: Partial<Record<Info, Info>> = { [Info.TagEnd]: Info.TagA1, [Info.TagA1]: Info.TagA2, [Info.TagA2]: Info.TagA3, [Info.TagA3]: Info.TagA4, [Info.TagA4]: Info.TagA5, [Info.TagA5]: Info.TagA6 };
      if (after[next] !== undefined) return [0, after[next]!];
    }
    if (isTagDigit(cp)) {
      if (next === Info.TagEnd || next === Info.TagA1 || next === Info.TagA2 || next === Info.TagA3 || next === Info.TagA4) return [0, Info.TagD1];
      if (next === Info.TagD1) return [0, Info.TagD2];
      if (next === Info.TagD2) return [0, Info.TagD3];
    }
    if (cp === 0x1f3f4 && (next === Info.TagA3 || next === Info.TagA4 || next === Info.TagA5 || next === Info.TagA6 || next === Info.TagD3)) return [0, Info.EmojiPresentation];
    if (next === Info.ZwjEp && lookup(cp)[1] === Info.EmojiPresentation) return [0, Info.EmojiPresentation];
  }
  return lookup(cp);
}

/** unicode-width's `UnicodeWidthStr::width` for `text` (control characters count 1; clean them first). */
export function stringWidth(text: string): number {
  const cps = [...text].map(c => c.codePointAt(0)!);
  let sum = 0, next = Info.Default;
  for (let i = cps.length - 1; i >= 0; i--) { const [w, info] = step(cps[i], next); sum += w; next = info; }
  return Math.max(0, sum);
}

const segmenter = new Intl.Segmenter(undefined, { granularity: 'grapheme' });
/** The grapheme clusters of `text`, each with the columns ratatui gives it. */
export function clusters(text: string): { text: string; width: number }[] {
  return [...segmenter.segment(text)].map(({ segment }) => {
    // ratatui counts a one-byte (ASCII) cluster as one column without a lookup.
    if (segment.length === 1 && segment.charCodeAt(0) < 0x80) return { text: segment, width: 1 };
    let marks = 0;
    for (const c of segment) if (c === 'ﾞ' || c === 'ﾟ') marks++;
    return { text: segment, width: stringWidth(segment) + marks };
  });
}
/** Terminal columns `text` takes once zero-width clusters are dropped. */
export function columns(text: string): number {
  return clusters(text).reduce((sum, c) => sum + c.width, 0);
}
