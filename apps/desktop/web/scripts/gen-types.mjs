// Regenerates src/ir/generated.ts from the Rust schema.
//   cargo run -q -p sky-ir --example schema > src/ir.schema.json && pnpm gen:types
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { compile } from 'json-schema-to-typescript';

const schema = JSON.parse(readFileSync(new URL('../src/ir.schema.json', import.meta.url), 'utf8'));
const defs = schema.$defs;

// schemars writes an internally tagged newtype variant as `{properties: {type}, $ref}`.
// json-schema-to-typescript turns that into two interfaces (`X` holding only the tag,
// `X1` holding the fields), so merge the referenced definition into the branch instead.
function inlineTaggedRefs(node) {
  if (Array.isArray(node)) return node.forEach(inlineTaggedRefs);
  if (!node || typeof node !== 'object') return;
  if (typeof node.$ref === 'string' && node.properties && node.$ref.startsWith('#/$defs/')) {
    const name = node.$ref.slice('#/$defs/'.length);
    const target = defs[name];
    if (target?.properties) {
      node.title = name;
      node.properties = { ...node.properties, ...target.properties };
      node.required = [...new Set([...(node.required ?? []), ...(target.required ?? [])])];
      delete node.$ref;
    }
  }
  Object.values(node).forEach(inlineTaggedRefs);
}
inlineTaggedRefs(schema);
// The merged branches replace these definitions; keeping them would emit `X1` duplicates.
for (const name of ['TemplateTarget', 'PointTarget']) {
  if (!JSON.stringify(schema).includes(`#/$defs/${name}"`)) delete defs[name];
}

const ts = await compile(schema, 'IrDoc', {
  bannerComment: '/* Generated from the Rust IR schema (crates/sky-ir). Do not edit; run `pnpm gen:types`. */',
  additionalProperties: false,
  unreachableDefinitions: true,
  style: { singleQuote: true, semi: true },
});
mkdirSync(new URL('../src/ir/', import.meta.url), { recursive: true });
writeFileSync(new URL('../src/ir/generated.ts', import.meta.url), ts);
console.log('wrote src/ir/generated.ts');
