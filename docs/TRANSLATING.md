# Translating Heim

Heim speaks French and English. Adding a language takes one file for the interface, and one
list for the messages sent by the Raspberry Pi.

## The interface

The code is written in French, and the French text *is* the key (like gettext):

```js
tr('Allumer')                       // "Allumer" in French, "Turn on" in English
tr('{n} lumières allumées', { n })  // placeholders between braces
```

1. Copy `ui/src/lib/lang/en.js` to `ui/src/lib/lang/<code>.js` (for example `de.js`).
2. Translate every **value** and keep the keys as they are. Keep `{placeholders}` unchanged; you
   can move them around in the sentence.
3. In `ui/src/lib/i18n.js`, import the file and add it to `LANGS` (with its name in its own
   language and its locale for dates and numbers, e.g. `de-DE`) and to `DICTS`.
4. Run `npm test` in `ui/`: it lists any text without a translation, and any translation whose
   placeholders don't match.

`node scripts/i18n-keys.mjs <code>` prints the missing texts, grouped by file.

A missing translation is never an error: the French text is shown instead.

## The Pi's messages

Error messages, the logbook and notifications are translated by `src/i18n.rs`: a list of
`("French pattern", "translation")` pairs, where `{}` stands for a variable part (a name, a
number). In the translation, `{}` takes the parts in order, or `{0}`, `{1}`… to reorder them.

Add a list for your language next to `EN`, and a line in `tr()`. `cargo test` checks that every
pattern has as many holes as its translation.

## What is not translated

Names you give yourself: devices, rooms, scenes, boards, the texts of your tile rules. Heim
shows them as you wrote them.
