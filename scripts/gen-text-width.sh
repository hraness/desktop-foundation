#!/bin/sh
# Prints the width tables for sdk/src/text-width.ts from the unicode-width
# crate that Cargo.lock pins. Run it after a unicode-width upgrade, paste the
# five constants over the old ones, and run `npm run check`: the shared corpus
# in contract/text-width.json then proves the two kits agree.
set -eu
VERSION=$(awk '/^name = "unicode-width"/{getline; print $3}' Cargo.lock | tr -d '"' | sort -V | tail -1)
cargo fetch --locked >/dev/null
SRC=$(ls -d "${CARGO_HOME:-$HOME/.cargo}"/registry/src/*/unicode-width-"$VERSION" | head -1)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/src"
printf '[package]\nname = "gen-text-width"\nversion = "0.0.0"\nedition = "2021"\n[workspace]\n' > "$WORK/Cargo.toml"
sed -e 's/^fn lookup_width(/pub fn lookup_width(/' \
    -e 's/^struct WidthInfo(u16)/pub struct WidthInfo(pub u16)/' \
    -e 's/^    const EMOJI_PRESENTATION: Self/    pub const EMOJI_PRESENTATION: Self/' \
    "$SRC/src/tables.rs" > "$WORK/src/tables.rs"
cat > "$WORK/src/main.rs" <<'RS'
#![allow(dead_code, unexpected_cfgs)]
mod tables;
use tables::*;
fn ranges(pred: impl Fn(char) -> bool) -> Vec<(u32, u32)> {
    let (mut out, mut start, mut last) = (Vec::new(), None, 0);
    for cp in 0..0x110000u32 {
        if char::from_u32(cp).is_some_and(&pred) {
            start.get_or_insert(cp);
            last = cp;
        } else if let Some(s) = start.take() {
            out.push((s, last));
        }
    }
    out.extend(start.map(|s| (s, last)));
    out
}
fn fmt(r: &(u32, u32)) -> String {
    if r.0 == r.1 { format!("{:x}", r.0) } else { format!("{:x}-{:x}", r.0, r.1) }
}
fn emit(name: &str, items: Vec<String>) {
    let text = items.join(",");
    let chunks: Vec<String> = text.as_bytes().chunks(110).map(|c| format!("  '{}'", String::from_utf8_lossy(c))).collect();
    println!("const {name} =\n{};", chunks.join(" +\n"));
}
fn main() {
    let mut w: Vec<(u32, String)> = Vec::new();
    for width in [0u8, 2, 3] {
        for r in ranges(|c| c >= '\u{a0}' && lookup_width(c).0 == width) {
            w.push((r.0, format!("{}:{width}", fmt(&r))));
        }
    }
    w.sort();
    emit("WIDTH", w.into_iter().map(|x| x.1).collect());
    let list = |p: fn(char) -> bool| ranges(p).iter().map(fmt).collect();
    emit("VS16", list(starts_emoji_presentation_seq));
    emit("VS15", list(starts_non_ideographic_text_presentation_seq));
    emit("MODBASE", list(is_emoji_modifier_base));
    emit("EP", list(|c| lookup_width(c).1 == WidthInfo::EMOJI_PRESENTATION));
}
RS
cargo run -q --offline --manifest-path "$WORK/Cargo.toml"
