#!/usr/bin/env node
// The interface's string tables (web/locales/*.json) against the interface (web/*.js, web/*.html). Run: node tools/check_locales.mjs
//  - every key the interface uses is in en.json, and every key in en.json is used (none left over);
//  - every language has exactly en.json's keys, each with the same {placeholders};
//  - plural values ({one, few, many, other}) have every form the language needs (Intl.PluralRules), and only those;
//  - no key is put together at run time (t('setup.' + x)): a table of whole keys keeps every key visible to this check;
//  - each translation other than English starts with "_review", the note that it needs a native speaker's review.
// Keys are words with dots, starting with one of NAMESPACES. Keys starting with "_" are notes, not text.
import { readFileSync, readdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const WEB = join(ROOT, 'web'), LOCALES = join(WEB, 'locales');
const LANGS = ['en', 'it', 'de', 'fr', 'es', 'pl'];
const NAMESPACES = ['lang', 'common', 'nav', 'setup', 'app', 'map', 't3d', 'chat', 'update'];
// What each translation says about its review, first in the file: the line-by-line review's note.
const REVIEW = 'reviewed line by line on 17 Nov 2026; a native speaker\'s read is still advised for first-aid wording';
const KEY = `(?:${NAMESPACES.join('|')})\\.[a-z0-9_]+(?:\\.[a-z0-9_]+)*`;

const errors = [], notes = [];
const err = m => errors.push(m);

// ---- what the interface uses
const files = readdirSync(WEB).filter(f => /\.(js|html)$/.test(f));
const used = new Map();   // key -> first place it is used
for (const f of files) {
  const src = readFileSync(join(WEB, f), 'utf8');
  const lineOf = i => src.slice(0, i).split('\n').length;
  for (const m of src.matchAll(new RegExp(`(['"\`])(${KEY})\\1`, 'g'))) if (!used.has(m[2])) used.set(m[2], `${f}:${lineOf(m.index)}`);
  for (const m of f.endsWith('.html') ? src.matchAll(/data-i18n-attr="([^"]*)"/g) : []) {
    for (const pair of m[1].split(';')) {
      const k = (pair.split('=')[1] || '').trim();
      if (!new RegExp(`^${KEY}$`).test(k)) err(`${f}:${lineOf(m.index)}: data-i18n-attr names "${k}", which is not a key`);
      else if (!used.has(k)) used.set(k, `${f}:${lineOf(m.index)}`);
    }
  }
  // a key made at run time: t('setup.' + code), t(`map.${x}`)
  for (const m of src.matchAll(new RegExp(`\\bth?\\(\\s*(?:(['"])(?:${NAMESPACES.join('|')})\\.[^'"]*\\1\\s*\\+|\`(?:${NAMESPACES.join('|')})\\.[^\`]*\\$\\{)`, 'g')))
    err(`${f}:${lineOf(m.index)}: a key put together at run time; use a table of whole keys`);
}

// ---- the tables
const read = lang => {
  const path = join(LOCALES, `${lang}.json`);
  let text;
  try { text = readFileSync(path, 'utf8') } catch { err(`web/locales/${lang}.json is missing`); return null }
  try { return { data: JSON.parse(text), text } } catch (e) { err(`web/locales/${lang}.json is not valid JSON: ${e.message}`); return null }
};
const holes = s => [...String(s).matchAll(/\{(\w+)\}/g)].map(m => m[1]);
const placeholders = v => new Set(typeof v === 'object' && v ? Object.values(v).flatMap(holes) : holes(v));
const same = (a, b) => a.size === b.size && [...a].every(x => b.has(x));

const en = read('en');
if (!en) { console.error(errors.join('\n')); process.exit(1) }
const EN = en.data, enKeys = Object.keys(EN).filter(k => !k.startsWith('_'));

for (const k of enKeys) {
  if (!new RegExp(`^${KEY}$`).test(k)) err(`en.json: "${k}" is not a key (words with dots, starting with ${NAMESPACES.join(', ')})`);
  if (!used.has(k)) err(`en.json: "${k}" is not used anywhere in web/`);
}
for (const [k, where] of used) if (!Object.hasOwn(EN, k)) err(`${where}: "${k}" is not in en.json`);

for (const lang of LANGS) {
  const table = lang === 'en' ? en : read(lang);
  if (!table) continue;
  const D = table.data, name = `${lang}.json`;
  const cats = new Set(new Intl.PluralRules(lang).resolvedOptions().pluralCategories);
  if (lang !== 'en') {
    const first = Object.keys(D)[0];
    if (first !== '_review' || D._review !== REVIEW) err(`${name}: must start with "_review": "${REVIEW}"`);
  }
  const keys = Object.keys(D).filter(k => !k.startsWith('_'));
  for (const k of keys) if (!Object.hasOwn(EN, k)) err(`${name}: "${k}" is not in en.json`);
  let sameAsEnglish = 0;
  for (const k of enKeys) {
    if (!Object.hasOwn(D, k)) { err(`${name}: "${k}" is missing`); continue }
    const v = D[k], e = EN[k];
    if (typeof e === 'object' && e) {
      if (typeof v !== 'object' || !v) { err(`${name}: "${k}" needs plural forms, as in en.json`); continue }
      const forms = Object.keys(v);
      for (const c of cats) if (typeof v[c] !== 'string' || !v[c].trim()) err(`${name}: "${k}" lacks the "${c}" form that ${lang} needs`);
      for (const c of forms) if (!cats.has(c)) err(`${name}: "${k}" has a "${c}" form, which ${lang} does not use`);
    } else if (typeof v !== 'string' || !v.trim()) { err(`${name}: "${k}" must be text`); continue }
    if (!same(placeholders(v), placeholders(e))) err(`${name}: "${k}" has {${[...placeholders(v)].join('}, {')}}; en.json has {${[...placeholders(e)].join('}, {')}}`);
    if (lang !== 'en' && JSON.stringify(v) === JSON.stringify(e)) sameAsEnglish++;
  }
  if (lang !== 'en') notes.push(`${name}: ${keys.length} keys, ${sameAsEnglish} the same as in English (names such as Lastbook, OpenStreetMap or "OK" may be)`);
}

const where = `${files.length} files in web/, ${used.size} keys used, ${enKeys.length} in en.json`;
if (errors.length) {
  console.error(errors.join('\n'));
  console.error(`\n${errors.length} problem${errors.length === 1 ? '' : 's'} (${where})`);
  process.exit(1);
}
console.log(notes.join('\n'));
console.log(`All locales match (${where}).`);
