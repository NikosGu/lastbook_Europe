# Curated topic files (`content/topics/<id>.json`)

Each file is one "need" in the app, written for people in Europe from the documents of the library packs
(`content/library/*.json` lists them). The app shows, in order:
1. the title and a one-sentence intro,
2. **choices** ("What's happening?") — the person taps one situation and sees 1–4 recommended readings for it,
3. **limits** — when the source itself says to stop, get help, or that a professional/trained person is needed,
4. **prepare** — readings for getting ready before it happens,
5. **learn** — whole documents for deeper study,
6. for learning tracks, an ordered **route** of beginner-to-advanced steps.

Source facts (publisher, edition, date, file type, page count, video length, transcript) are filled in
automatically from `catalog.json` — do not copy them into the topic file.

```json
{
  "id": "water",
  "mode": "now",                     // "now" = urgent need, "learn" = longer-term learning track
  "title": "Get safe drinking water",
  "short": "Store, treat and keep water clean",
  "intro": "One or two plain sentences in your own words. Describe what this section helps with; do not give procedures.",
  "keywords": ["water", "boil", "chlorine", "purify", "filter", "thirsty", "drinking"],
  "choices": [
    {
      "id": "unsafe",
      "label": "Tap water is off or might be unsafe",
      "hint": "Flood, broken pipe, a warning not to drink it",
      "read": [
        {"doc": "<catalog id>", "page": 3,
         "label": "Short action-style headline of what the page helps with",
         "note": "What the reader will find on that page, <= 30 words, describing (not prescribing)."}
      ]
    }
  ],
  "limits": [
    {"text": "Faithful short paraphrase of a limit/warning/when-to-get-help statement from the source.",
     "doc": "<catalog id>", "page": 1}
  ],
  "prepare": [ {"doc": "...", "page": 3, "label": "...", "note": "..."} ],
  "learn":   [ {"doc": "...", "label": "...", "note": "..."} ],
  "route": [
    {"title": "Step 1: ...", "goal": "What the step is for, in plain words",
     "read": [ {"doc": "...", "page": 3, "label": "...", "note": "..."} ]}
  ],
  "related": ["sanitation", "food"]
}
```

Rules
- Every `doc` must be the id of a document a pack lists in `content/library/*.json`. `page` is the PDF *file* page,
  counted from 1, as the reader opens it (`#/read/<id>?p=<page>`); omit it for a whole document. Verify each page by
  reading it in the built library (`library-data/text/<id>.txt`, pages separated by a form feed).
- `label`/`note`/`intro`/`hint` are plain, calm language for a stressed non-expert. They may describe what a page contains,
  but must not add steps, doses, quantities or advice that the page does not say. Nothing from memory: what the library
  does not cover goes in `content/gaps.json` (`missing`), and sources that disagree go there too (`disagree`).
- `limits` must be traceable to the cited page. Paraphrase faithfully; keep any quote under 15 words.
- Europe: the emergency number is 112 (name a national number only as the cited page does), units are metric, and the
  hazards are Europe's (floods, heat, cold, blackouts, earthquakes, storms, forest fires).
- Medical, chemical (chlorine, fuels), electrical and gas items: prefer current WHO, IFRC and Red Cross, and national
  civil-protection sources. Say in `note` when a source is old, is written for trainers, health workers or relief
  planners, or belongs to one country (its numbers, sirens or offices).
- A `note` says when the document is in another language than the guide ("In German.").
- Favour fewer, better items: 1–4 readings per choice, 3–8 in prepare, 4–10 in learn.
- A guide the library cannot support yet keeps its title, a short honest intro, its keywords (so a search still finds
  it) and no readings; `content/gaps.json` says what is missing. Never fill it with documents from elsewhere.
- A pack may hold fewer documents than the guides name: the pack that may be published holds only those whose terms let
  them be passed on (`"redistribute": "yes"` in `content/library/starter.json`). A reading, warning, preparing or
  learning item whose document is not in the library is not shown, and a step left with nothing to open goes. A guide
  left with none of its documents says so in place of its short line and intro, and one left with some of them says
  after its intro that the others are not here, in the words of `content/uncovered.json` (one entry for each interface
  language).

Other languages (`content/topics/<lang>/<id>.json`)
- The app asks for the guides in the interface's language (`/api/topics?lang=it`) and takes each guide from
  `content/topics/<lang>/` where it is there, from English otherwise. The English files decide which guides there are.
- A translation has the English guide's items: the same choices (same ids), the same readings, warnings, preparing and
  learning items in the same order, and the same `related`. Each item opens the English item's page, or a page of a
  document in the translation's own language that says the same thing (a warning then paraphrases that page). A
  document's edition in another language counts as the same pages where its sources list says `same_pages_as`. The
  `note`s say when a document is in another language than the guide.
- It starts with `"_review"`, the same note as `web/locales/` carry: now "reviewed line by line on 17 Nov 2026; a
  native speaker's read is still advised for first-aid wording".
- It carries `"gaps"`: the guide's entry of `content/gaps.json` (`missing`, `disagree`) in its own words, item for item,
  each note with the same `docs`.
- A gap note is `{"text": "...", "docs": [...]}`: `docs` are the ids of the documents it names (`[]` for one that names
  none). A note that names a document the library does not hold is not shown, as a reading or a warning is not.
- Where a translated item opens a page of a document in its own language and the library lacks that document, the item
  opens the English item's page instead, if that document is there; it is hidden only when neither is.
- `cargo test -p lastbook-engine topics` checks all of this.
