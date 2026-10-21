# The Kokoro front end and the speaker check bar

Research note, 2026-10-21, branch `docs/voice-risks`. Research only: repositories, model cards, papers and package registries. No model was downloaded or run, and no audio was recorded.

[BUILD.md](../BUILD.md) section 7 lists two open voice risks, and this note answers both:

1. "The espeak-free speech build and Eileen's own Kokoro front end are new work; if the G2P model's quality is poor, Kokoro's pronunciation suffers." Section 2.8 ("No GPL in the speech build") plans a CMUdict lexicon plus a small permissively licensed ONNX grapheme-to-phoneme (G2P) model, mapped to Kokoro's phonemes and run through `ort`.
2. The speaker check bar (owner direction 2026-10-21; BUILD.md 2.8 and WP4 acceptance 13). A speaker check may gate anything only if, on the owner's consented command-length speech (one to three seconds) in four room conditions, the one-sided 95 percent Clopper-Pearson upper bounds of both the false-reject and the false-accept rate are under 1 percent, on at least 300 held-out owner utterances and 300 by at least five other people.

It is written for whoever builds `eileen-speech` in WP4, and for the owner, who has three questions to answer (section 5).

## Why Eileen avoids the GPL phonemizer

The owner asked on 2026-10-21 why the GPL phonemizer, espeak-ng, was avoided. The short answer: **Eileen's releases carry no GPL code, by the owner's own rule.** Owner answer 12 in [OWNER-DIRECTION.md](../OWNER-DIRECTION.md) (2026-10-20) reads "No GPL in release binaries: use a TTS front end without espeak-ng", and BUILD.md 2.8 applies it.

The reasoning behind the rule:

- Eileen is MIT-licensed (the repository's `LICENSE`). espeak-ng "is released under the GPL version 3 or later license", in its own README's words [primary].
- espeak-ng would not be a separate program Eileen runs now and then; it would be built into Eileen. The upstream sherpa-onnx 1.13.8 library compiles it in for its TTS front end, and Kokoro's reference pipeline has misaki, Kokoro's G2P library, call it for every word misaki's dictionaries lack (section 1) [primary]. The FSF's GPL FAQ says of a program that links a GPL library: "the terms of the GPL apply to the entire combination" [primary]. Every Eileen release would then be a GPL-3.0 combination: anyone passing Eileen on would owe the GPL's source and licence terms, and the MIT terms would hold for the source but not for what people install.
- BUILD.md enforces the rule mechanically: the release lane's licence listing fails on any GPL component (4.6, and WP4 acceptance 11 checks the native archive for espeak-ng symbols and data).

espeak-ng is still fine where nothing ships, for example as a reference to compare against during development.

**Avoiding it costs little, and the cost is falling.** misaki now has a GPL-free model for unknown words, and one third-party test puts it level with espeak-ng on names (16.77 against 17.46 percent phoneme error rate). The same project's GPL-free Kokoro front end scored 2.42 percent word error rate when its speech was transcribed, against 2.28 percent for misaki with espeak-ng [secondary, unreviewed]. sherpa-onnx itself removed espeak-ng on 2026-10-20, and the change describes the library as "GPL-free" from then on [primary]. Section 2 has the details.

## How to read this

- **[primary]**: the project's own repository, source code, model card, release record, registry entry or paper, opened on 2026-10-21.
- **[secondary]**: a third party's measurement. Most come from loqui, a small MIT-licensed Rust project published in late September 2026 with no outside review yet, and are marked "unreviewed".
- **[ran 10-21]**: computed for this note on 2026-10-21. That means entry counts and word lookups in two pronunciation lexicons, read as plain data files with `python -I`; the BART model's parameter count from its `config.json`; and the binomial bounds of section 3.1. No model was involved.
- **[inferred]**: our reasoning, not a measurement.

The PC is the one described in [2026-10-voice-stack.md](2026-10-voice-stack.md) (Core Ultra 7 155H, 16 GB). Other agents were building on it, and free memory stayed between about 0.5 and 2.6 GB during this research, one more reason nothing heavy was run.

## Verdict

**G2P: the risk is smaller than BUILD.md assumed, and the plan should change in four places.**

1. **Kokoro's own G2P already has a GPL-free model for unknown words, so Eileen should use it rather than invent one.** misaki (Apache-2.0) made a small character-level BART model its default fallback on 2025-04-14 (pull request #74): about 752k parameters, a 3,011,692-byte weights file, Apache-2.0, writing Kokoro's phoneme alphabet directly, so no mapping is needed [primary; parameter count ran 10-21]. It is on misaki's main branch only; the last PyPI release, 0.9.4 of 2025-04-05, predates it [primary]. One fact matters for data provenance, though not for the GPL rule: the published weights were trained on misaki's gold lexicon and 90 percent of its silver lexicon, and silver is espeak-ng output (section 2.1) [primary]. The weights contain no GPL code.
2. **sherpa-onnx removed espeak-ng on 2026-10-20.** Pull request #4055 removes espeak-ng and piper-phonemize and says "sherpa-onnx is GPL-free after this PR". The maintainer announced the change as major release 2.0.0 (issue #3731) and wrote on 2026-10-16 that the team was "working hard to try to release it next week"; crates.io's newest is still 1.13.8, of 2026-09-11 [primary]. The TTS generation config now accepts pre-phonemized input (`phoneme_codepoints` or `tokens`) in every binding, Rust included, and a word missing from a Kokoro lexicon is skipped with "OOV word skipped" instead of going to espeak-ng [primary]. Once 2.0.0 is released and 14 days old, WP4's own from-source build is probably unnecessary (section 4).
3. **The lexicon should be misaki's gold dictionary plus CMUdict, not CMUdict alone.** misaki's `us_gold.json` has 90,201 entries, 790 of which change with the part of speech ("read" is ɹˈid by default and ɹˈɛd as a past tense), all in Kokoro's alphabet [ran 10-21]. The assistant's own name is in neither of misaki's lexicons, while CMUdict (126,052 words, a BSD-style licence) has "eileen" as `AY0 L IY1 N` [ran 10-21; re-read in the file on 2026-10-21]. So CMUdict belongs in a second layer, mapped into Kokoro's alphabet, between gold and the neural model. misaki's other lexicon, silver (93,361 entries), is espeak-ng 1.52 output run through transforms, in the maintainer's own words [primary], and should stay out of releases (section 5, question 1).
4. **The ceiling on unknown words is set by English spelling, not by the choice of model.** Strong published word-level models still get about one held-out CMUdict word in five not exactly right (a Transformer of about 2M parameters: 5.23 percent phoneme error rate, 22.1 percent word error rate) [primary], and names are harder. Names Eileen says often (the assistant's name, app names, voice names, contacts the user teaches) need a shipped names list and a user lexicon that "say it like ..." writes to.

**Recommendation for the front end.**

- **Primary:** Eileen's own Rust port of misaki's English pipeline (tokenizer, part-of-speech tagger, gold lexicon with its part-of-speech entries, suffix rules, and number reading written from scratch), with Eileen's names list and the user lexicon in front of gold, a CMUdict layer after it, and misaki's published BART model (`PeterReid/graphemes_to_phonemes_en_us` at revision `a5631b285d18d59483c32c0c3379cb9fac924f4b`) for words no layer knows.
- **Fallback:** the same BART architecture with weights Eileen trains itself on gold plus CMUdict mapped into Kokoro's alphabet, with no espeak-derived data and a held-out test set. It uses the same runtime code with different weights. It replaces the primary's weights if WP4's held-out tests (2.5) show it better on unknown words and names, or if the owner rules out models trained on espeak output (section 5, question 1).
- **Depend on neither `loqui-g2p` nor `misaki-rs`; measure against loqui.** Both carry espeak-derived data: loqui embeds misaki's silver lexicon, and misaki-rs extended its dictionary "using eSpeak" and turns an espeak-ng fallback on by default [primary]. loqui is also a three-week-old project with no outside users.

Section 2.6 gives the reasons.

**Speaker check: the bar is very unlikely to be met on short commands, and WP4 should plan for `passed = false`.** Published text-independent models small enough for this CPU reach 0.3 to 0.9 percent equal error rate (EER) on full-length VoxCeleb trials, the newest (ReDimNet2) at the low end. With full-length enrollment and a 2 s test they are at 1.5 to 2.1 percent, and with 1 s tests at about 3 to 5 percent [primary]. Far-field and multi-genre sets cost 2.7 to 9.6 percent even with long tests, and three interfering talkers mixed into each trial take a 0.88 percent system to 12.84 percent [primary]. The bar asks for far less error than any of that. With 300 trials per side it allows no error at all, and to pass with even odds the true rates must be near 0.2 percent on both sides at once, pooled over all four room conditions (section 3.1). A pass in the quiet desk condition with long, multi-session enrollment is conceivable; across the room and with another person talking, the published evidence says no. BUILD.md already makes that outcome safe: with `passed = false` the gate is inert and nothing is refused for voice. Section 3.4 gives the enrollment and test design that keeps the measurement honest and cheap, starting with a ten-minute pilot in the hardest condition.

## 1. What Kokoro expects from a front end

### 1.1 The phoneme alphabet

misaki's `EN_PHONES.md` says that "for English, Misaki currently uses 49 total phonemes": 41 shared symbols (the stress marks ˈ and ˌ, 22 IPA consonants, the affricates ʤ and ʧ, ten IPA vowels, four capital-letter diphthongs, A for eɪ, I for aɪ, W for aʊ and Y for ɔɪ, and a made-up ᵊ, a small muted schwa), four American-only symbols (æ, O for oʊ, ᵻ and the flap ɾ) and four British-only ones (a, Q for əʊ, ɒ and ː) [primary]. Kokoro-82M's `config.json` vocabulary has 115 entries and contains misaki's invented symbols (A, I, O, Q, W, Y, ᵊ, ᵻ) [primary]. No other G2P writes those, so Kokoro was evidently trained on misaki's alphabet [inferred]; its model card says only that it was trained on "IPA phoneme labels" and that the `kokoro` package uses misaki [primary]. Stress marks sit immediately before the stressed vowel ("lead" is lˈid) [ran 10-21]. One phoneme chunk holds at most 510 symbols in Kokoro's pipeline [primary].

### 1.2 What Kokoro's own pipeline does

`KPipeline` in hexgrad/kokoro (Apache-2.0) builds `en.G2P(trf=trf, british=..., fallback=fallback, unk='')`. `fallback` is misaki's `EspeakFallback` if espeak-ng loads; otherwise it is `None`, and the pipeline logs "EspeakFallback not Enabled: OOD words will be skipped" [primary]. What `None` means depends on the misaki version, and Kokoro only asks for `misaki[en]>=0.9.4` [primary]:

- With PyPI's 0.9.4, `None` means no fallback, and with `unk=''` an unknown word is dropped silently.
- On misaki's main branch, `G2P` replaces a missing fallback with the BART model (`self.fallback = fallback if fallback else FallbackNetwork(british)`) [primary].

So Kokoro's reference pipeline uses espeak-ng whenever it can load it, and reaches the GPL-free model only on misaki's unreleased main branch. Kokoro's other languages (codes e, f, h, i and p: Spanish, French, Hindi, Italian and Brazilian Portuguese) go through espeak-ng entirely (`EspeakG2P`) [primary], so a Kokoro voice in those languages has no GPL-free front end today. That does not touch v0.1, which converses in English, but it matters for ROADMAP v0.2 (owner answer 21).

misaki's English G2P (`misaki/en.py` on main) works in six steps [primary]:

1. spaCy's `en_core_web_sm` (MIT) tokenizes and tags, loaded with only `tok2vec` and `tagger` enabled; `trf=True` uses `en_core_web_trf` instead.
2. Special cases for function words whose sound depends on the tag or the next sound: "a" (ɐ as a determiner, ˈA otherwise), "the" (ði before a vowel, ðə otherwise), "to" (tə, tʊ or the dictionary form) and "used" (the past-tense form before "to").
3. Lexicon lookup, gold before silver (silver is skipped for all-capital proper nouns), choosing the entry for the exact Penn tag, then its parent tag (VERB, NOUN, ADV or ADJ), then `DEFAULT`.
4. Suffix rules: strip -s, -ed or -ing, look up the stem, and add the right ending back (z, s or ᵻz; t, d or ᵻd, with ɾᵻd after a flapped t; ɪŋ, or ɾɪŋ after a flapped t).
5. Numbers, currencies, ordinals and years through `num2words`, and symbols and dotted abbreviations through tables.
6. Anything still unknown goes to the fallback, and failing that to the `unk` marker (❓ by default).

The `misaki[en]` extra installs `num2words`, `spacy`, `spacy-curated-transformers`, `phonemizer-fork`, `espeakng-loader`, `torch` and `transformers` [primary]. `num2words` is LGPL (PyPI gives no version) [primary] and `phonemizer-fork` wraps espeak-ng, so Eileen's number reading should be written from its own specification, not ported from `num2words` [inferred]. misaki's repository has had no push since 2025-08-11, and the lexicon issues filed this year (#105 and #107) have no maintainer reply [primary]. A port is therefore Eileen's to maintain; there is no active upstream to follow.

### 1.3 What sherpa-onnx does with Kokoro

The `kokoro-multi-lang-v1_0` package that BUILD.md 4.2 pins carries `lexicon-us-en.txt` (5.6 MB), `lexicon-gb-en.txt` (6.0 MB) and an `espeak-ng-data` directory [primary]. sherpa-onnx's `generate_lexicon_en.py` builds the English lexicons by merging misaki's silver and gold dictionaries (gold wins), keeping only each word's `DEFAULT` pronunciation, lowercasing every word and adding "Kokoro" (kˈOkəɹO) and "Misaki" by hand [primary]. So the shipped lexicon contains espeak-derived silver entries and has lost the part-of-speech entries. In 1.13.8, `KokoroMultiLangLexicon` sends a word missing from the lexicon to espeak-ng ("fallback to espeak if a word is not in the lexicon"). On master after pull request #4055, the Kokoro lexicon path logs "OOV word skipped" and moves on [primary].

BUILD.md 2.8 already runs Kokoro through `ort` with Eileen's own front end, not through sherpa-onnx's TTS. So the sherpa-onnx change matters for the native build (section 4), not for how Kokoro speaks.

## 2. Grapheme-to-phoneme options without GPL code

### 2.1 The candidates

| Option | Licence (code / weights / data) | Size | Output | Published accuracy | CPU cost | Names and unknown words | Rust or ONNX readiness | Verdict |
|---|---|---|---|---|---|---|---|---|
| misaki's BART fallback (`PeterReid/graphemes_to_phonemes_en_us`, `_en_gb`) | Apache-2.0 (Hugging Face licence tag; the model card is a 31-byte stub) / trained on misaki's gold lexicon, 90 percent of its silver lexicon and generated plurals, per the commit "Improve and shrink model" and the training script uploaded with the model | about 752k parameters (d_model 128, one encoder and one decoder layer, feed-forward 1,024, 64 positions, 63 symbols) [ran 10-21]; `model.safetensors` 3,011,692 B | Kokoro's alphabet, with stress | None from the author. The script holds out every tenth silver entry for evaluation, but no score is published. Third party: 16.77 percent phoneme error rate (PER) on 500 CMUdict names misaki lacks, against 17.46 for espeak-ng; a Rust port matches the PyTorch model on 500 of 500 words [secondary, unreviewed] | about 0.4M multiply-adds per output phoneme plus about 0.33M per input letter, so well under 1 ms per word [inferred] | One word at a time, no context, no confidence score; words over about 62 letters must be split | A plain Rust forward pass (loqui does it), a community ONNX export (`konecth/graphemes_to_phonemes_en_us_onnx`, Apache-2.0, 2025-12-15), or Eileen's own pinned export | **Primary** |
| espeak-ng (misaki's `EspeakFallback`, phonemizer, piper-phonemize) | GPL-3.0-or-later | small | espeak IPA, mapped by misaki | The usual reference | fast | Rules plus exception lists | The `espeak-ng` crate 0.2.0 (2026-09-05) is a pure Rust port, still GPL-3.0-or-later; `espeak-rs` is MIT bindings to the GPL library | Excluded (owner answer 12); a development reference only |
| OpenPhonemizer (NeuralVox) | BSD-3-Clause-Clear code / weights' licence not stated ("may be licensed under different licenses") / trained on `mrfakename/ipa-phonemes-word-pairs`, CC-BY-SA-4.0, 274,926 pairs "generated using phonemizer/espeak" | DeepPhonemizer models; the variant with a built-in dictionary is about 100 MB larger | espeak-style IPA | PhonemizerBench, scored as agreement with espeak: 93.52, against g2p_en 86.06, DeepPhonemizer 84.97 and gruut 74.78 | not given | Dictionary, then the model | No ONNX from the authors | No: it imitates espeak by design, its data is share-alike and espeak-made, and the repository was archived on 2026-03-15 as no longer maintained |
| DeepPhonemizer (as-ideas) | MIT / CMUdict (ARPAbet or IPA) and WikiPron models | not stated | ARPAbet or IPA | "comparable to state-of-art", with no numbers | not given | Dictionary first, Transformer for unknown words, a confidence per prediction | TorchScript export documented, ONNX not; last release 0.0.19 on 2023-02-27 | Possible, but stale, and it needs an export and the mapping of 2.4 |
| CharsiuG2P | MIT code / model licence not stated / mixed sources, including eSpeak NG output | ByT5, from tiny (8 layers, `pytorch_model.bin` 29,149,085 B) to small | IPA | Its own `eng-us` results: PER 14.6, 12.6 and 10.0 percent, WER 47.0, 41.4 and 35.6 percent (tiny 8 layers, tiny 16 layers, small) | floravox's ByT5 exports: about 64 ms per word for tiny int8 (18 MB), about 150 ms and 1.2 GB for small [secondary; floravox does not say whether they are CharsiuG2P's checkpoints] | Single words with a language prefix | No ONNX from the authors | No for English; weak and partly espeak-made |
| g2p_en (Kyubyong) | Apache-2.0 / the neural model's training data is not stated | GRU seq2seq, `checkpoint20.npz` 3,342,298 B | ARPAbet | None published | numpy greedy decoding, well under 1 ms per word [inferred] | CMUdict, 371 part-of-speech homographs (`homographs.en`), the GRU for the rest | Trivial to port; last commit 2019-12-31 | Stopgap only (2.6) |
| SoundChoice (SpeechBrain) | Apache-2.0 / LibriG2P (LibriSpeech alignments, Wikipedia homographs, CMUdict) | LSTM encoder, GRU decoder, BERT word embeddings | ARPAbet | 2.65 percent PER on LibriSpeech sentences, 94 percent homograph accuracy | needs a BERT model per sentence | Sentence-level, so homographs in context | PyTorch | No: too heavy for a live front end |
| Phonetisaurus | BSD-3-Clause / trains on any lexicon | WFST; "a few MB", about 70 MB in memory per language in floravox's builds [secondary] | the lexicon's alphabet | Joint-sequence models of this family score 5.88 percent PER and 24.53 percent WER on CMUdict [primary] | about 1 ms per word [secondary] | n-gram over grapheme-phoneme pairs | `phonetisaurus-g2p` crate (MIT, 0.1.1, 2025-04-08) | Considered; larger, and an older method |
| gruut | MIT | CRF | IPA | PhonemizerBench 74.78 | | | Archived (last push 2024-11-15) | No |

Sources for every row are listed at the end. "Joint-sequence" refers to Table 3 of Yolchuyeva et al., which also gives the Transformer figures in the verdict [primary].

### 2.2 Rust crates published this year

None of these existed in a usable form when BUILD.md was written, and all are young and lightly used. Read their own numbers as unreviewed.

| Crate | Licence | Newest release | What it is | Note |
|---|---|---|---|---|
| `loqui-g2p` (Awakened-Labs/loqui; repository created 2026-09-30, no stars) | MIT | 0.4.0, 2026-10-20; first release 0.1.0, 2026-10-01; 0.3.0 on 2026-10-04 | A Rust port of misaki's English pipeline. It embeds misaki 0.9.4's gold and silver lexicons unmodified, both BART models at pinned revisions (the en_us one at `a5631b2`), and its own averaged-perceptron tagger trained on public-domain Gutenberg text and generated sentences labelled by spaCy's `en_core_web_sm` 3.8.0. Dependencies: `serde`, `serde_json`, `fancy-regex`, `unicode-normalization`, `libm`, `thiserror`. The 0.4.0 crate is 9,318,022 B | Its parity log against Python misaki 0.9.4 with espeak-ng: PER 0.01 percent on the 720 Harvard sentences, 0.72 percent on a held-out assistant set of 50 lines, 0.78 percent on 4,930 Gutenberg lines; tokenizer identical to spaCy over 1.83M tokens; number words identical to `num2words` 0.5.14 on 7,026 cases; tagger agreement with spaCy 96.1 and 95.9 percent. End to end, over 250 lines transcribed by Whisper large-v3-turbo: 2.42 percent WER, against 2.28 percent for misaki with espeak-ng [secondary, unreviewed] |
| `misaki-rs` | MIT | 0.6.0, 2026-09-02 | A Rust port of misaki | The default feature `espeak` pulls `espeak-rs`. The README says its author "updated the original pronunciation dict ... using eSpeak". Without espeak, unknown words are spelled letter by letter, and a `Fallback` trait lets a caller plug in a model. loqui's parity log measures it at 11.52 percent PER on the assistant set and 0.52 percent on the Harvard sentences [secondary] |
| `floravox-g2p` (AACTools) | Apache-2.0 OR MIT | 0.8.8, 2026-10-14 | Memory-mapped FST lexicons with pluggable fallbacks (a Phonetisaurus WFST, ByT5 ONNX models, letter spelling). It ships no lexicon data; its English dictionaries come through an optional `misaki-rs` dependency | Inherits misaki-rs's eSpeak-extended data; pins `ort` =2.0.0-rc.14, newer than Eileen's =2.0.0-rc.13 |
| `open-english-pronouncing-dictionary` | CC-BY-SA-4.0 | 0.1.0, 2026-05-13 | A fused US English lexicon (misaki, CMUdict, WikiPron) of about 280,000 words | Share-alike data |
| `voicegarden-lexicons` (AACTools) | Apache-2.0 OR MIT code; MIT (gruut) and BSD-style (CMUdict) data | 0.3.0, 2026-09-01 | Fetches 14 lexicon bundles (13 languages, mostly from gruut, plus a CMUdict English bundle); its README says "No GPL data is included" | Offered on sherpa-onnx issue #3731 (2026-08-20) as an espeak-ng replacement; a candidate for v0.2's other languages |
| `sayd-misaki-en` | Apache-2.0 | 0.1.0, 2026-08-13 | "English grapheme-to-phoneme for Kokoro TTS, ported from misaki" | No repository listed |

`loqui-g2p` is close to what WP4 has to build, and its parity method is the right test: Python misaki as the oracle, public-domain sentences, a held-out assistant set, and a list of CMUdict words misaki lacks. **Recommendation: Eileen writes its own port and uses loqui as the measuring stick, not as a dependency** [inferred]. The reasons: loqui embeds the silver lexicon, which this note recommends keeping out of releases (section 5, question 1); it is three weeks old, from an organisation with no track record; it puts `fancy-regex` in the hot path; and it carries 9.3 MB of embedded data that Eileen would have to audit anyway. In its favour, it exists, and 0.3.0 is already past the two-week rule, so it is a useful oracle in development tests.

### 2.3 Names and unknown words

Lookups in misaki's lexicons (data last changed in commit `e820629`, 2025-04-05) and CMUdict's `cmudict.dict` (last changed in commit `0f8072f`, 2024-12-17) [ran 10-21]:

| Word | misaki gold | misaki silver | CMUdict |
|---|---|---|---|
| Eileen | missing | missing | `AY0 L IY1 N` |
| GitHub | missing | missing | `G IH1 T HH AH0 B` |
| DaVinci | missing | missing | `D AH0 V IH1 N CH IY2` |
| Resolve | ɹəzˈɔlv | missing | `R IY0 Z AA1 L V` |
| Claude | klˈɔd | klˈɔd | not checked |
| Kokoro | missing | missing | missing |
| Nemotron | missing | missing | missing |
| read | five entries by tag; ɹˈid by default | missing | two entries |
| lead | lˈid only (misaki issue #105, open: the metal sense is missing) | missing | two entries |

With misaki's lexicons alone, Eileen's own name goes to the neural fallback on every reply that says it [ran 10-21]. The same happens to many app names a user says, and to any name the user gives the assistant ("Eileen, I want to call you Nova now", BUILD.md E14). The word-level ceiling makes a better model the wrong lever. On CMUdict, a Transformer of about 2M parameters gets 22.1 percent of held-out words wrong somewhere (5.23 percent PER), and a joint-sequence model 24.53 percent (5.88 percent PER) [primary]. Names are harder still: 16.77 percent PER for the BART model and 17.46 percent for espeak-ng on 500 names [secondary]. The levers are layers and learning [inferred]:

1. **User lexicon first.** `vocabulary` facts in the user model (BUILD.md 2.8) also carry a pronunciation. "Say Kokoro like co-co-ro", a spelled-out respelling or a pick from two candidates writes an entry, versioned and undoable like every setting.
2. **Eileen's shipped names list:** the assistant's default name and the names the user picks, the launch grammar's app names, Kokoro's voice names, and the product names Eileen says in its own phrases. Each entry is reviewed by ear once and kept in a small committed file. sherpa-onnx's hand entry for "Kokoro" (kˈOkəɹO) is a seed.
3. **misaki gold**, with its part-of-speech entries.
4. **CMUdict mapped into Kokoro's alphabet** (2.4), for words gold lacks.
5. **misaki's suffix rules** on the stem.
6. **The BART model.**
7. **Letters spelled out** for all-capital tokens and anything the model cannot take (misaki's acronym rule).

misaki silver is not a layer: it is espeak-ng output (section 5, question 1), and the BART model already learned from it. A word that reaches layer 6 in a reply is also a candidate for the user lexicon, and the HUD can offer "how do I say this?" once for a word Eileen keeps meeting [inferred].

### 2.4 Mapping CMUdict into Kokoro's alphabet

ARPAbet maps nearly one to one onto misaki's American set [inferred, to be checked as below]:

| ARPAbet | Kokoro | ARPAbet | Kokoro | ARPAbet | Kokoro |
|---|---|---|---|---|---|
| AA | ɑ | EY | A | OW | O |
| AE | æ | IH | ɪ (often ᵻ when unstressed before a final consonant) | OY | Y |
| AH0 | ə | IY | i | UH | ʊ |
| AH1, AH2 | ʌ | ER1, ER2 | ɜɹ | UW | u |
| AO | ɔ | ER0 | əɹ | AW, AY | W, I |
| EH | ɛ | CH, JH | ʧ, ʤ | DH, TH | ð, θ |
| HH | h | NG | ŋ | R | ɹ |
| SH, ZH | ʃ, ʒ | Y | j | G | ɡ |

The other consonants keep their letter. Stress digits 1 and 2 become ˈ and ˌ placed just before the vowel. misaki's American set also writes allophones that CMUdict does not. The flap ɾ is used for t or d between a vowel and an unstressed vowel ("water" is wˈɔɾəɹ in gold [ran 10-21]). The custom ᵊ marks a syllabic consonant ("gimble" comes out as ɡˈɪmbᵊl in misaki pull request #74's examples [primary]). A handful of rules covers them. The check is cheap and needs no model: map every CMUdict word that is also in gold and report the agreement, before and after the allophone rules. WP4 adopts the CMUdict layer only if the agreement is high (for example 95 percent of words exact after normalising stress), and the disagreements show what the rules miss [inferred].

### 2.5 How quality gets measured in WP4

All of these run in CI on text, with no recordings and no GPL code in anything shipped [inferred]:

1. **Parity:** Eileen's front end against Python misaki (main branch, with the BART fallback) on the 720 Harvard sentences (public domain) and an assistant-style set of numbers, money, times, dates, acronyms and homographs. The target is under 1 percent PER, normalised for stress and flaps, with every difference listed. loqui's published figures are the bar to match.
2. **Unknown words:** 500 or more CMUdict words absent from gold, mapped as in 2.4, plus a names set (first names, app and product names, place names), scored as PER and WER for the published BART weights and for Eileen's retrained fallback. Words used to train the fallback are excluded from both sets. The weights with the lower WER on names, without being worse on the CMUdict set, are the ones that ship.
3. **Eileen's own words:** every entry in the shipped names list round-trips exactly.
4. **Intelligibility:** Kokoro speaks the assistant set and the names set through Eileen's front end, Nemotron transcribes it, and WER is compared with the text. This finds the errors a listener would notice. It needs Kokoro and Nemotron loaded, so it runs on a CI runner or in the exclusive slot, never alongside a build on the owner's PC.

### 2.6 Recommendation: one primary and one fallback

**Primary: misaki's pipeline in Rust, with misaki's published BART model for unknown words.**

- It is the front end Kokoro was built against, so the alphabet, the stress placement and the allophones match without a mapping.
- The BART model is Apache-2.0, 3 MB, and cheap enough to run on every unknown word inside the first clause's budget. It has been misaki's own default since 2025-04-14, and the one independent measurement puts it level with espeak-ng on names [secondary].
- It fits BUILD.md 2.8 as written ("a small permissively licensed ONNX grapheme-to-phoneme model for unknown words, mapped to Kokoro's phoneme set and run through `ort`"), except that no mapping is needed. WP4 either exports the encoder and one decoder step to ONNX once with a pinned script (as for AudioSeal) or runs the forward pass in plain Rust. Either way the weights are the same pinned `model.safetensors` (en_us revision `a5631b285d18d59483c32c0c3379cb9fac924f4b`; the file last changed in commit `746f1a6`, 2025-04-14), and the choice between the two runtimes is made by measurement.
- It meets owner answer 12: the weights are a learned function with no GPL code in them, and their publisher licenses them Apache-2.0. Their training data included misaki's silver lexicon, which is espeak-ng output. The FSF's FAQ says "the output of a program is not, in general, covered by the copyright on the code of the program" [primary], and weights trained on such output are a further step removed [inferred]. NOTICE says where the weights came from and what they were trained on.

**Fallback: the same BART architecture, retrained by Eileen on gold plus CMUdict mapped into Kokoro's alphabet.** It covers the two ways the primary can disappoint:

- **Quality.** The published model has no published held-out score. A retrain on CMUdict's 126,052 words plus gold, with a proper held-out split, gives a measured number, and CMUdict has many more names than gold, which is where the errors concentrate. Section 2.5, item 2 decides between the two.
- **Provenance.** It contains no espeak-derived input. Gold's own sources are still unnamed: the maintainer says gold was seeded from "some local datasets of IPA labels" and filtered with heuristics, and a request to name those sources (misaki issue #107, opened 2026-09-19) has no reply [primary]. If the owner wants no unexplained data at all, a CMUdict-only retrain is the variant to build.

It needs no new runtime code: same shape, same alphabet, same inference path, different weights. The training script is published with the model, and a 752k-parameter model trains in minutes on a GPU or in a longer CI job on CPU [inferred]; the owner's Colab is an option only with the owner's go-ahead. g2p_en's GRU (Apache-2.0, CMUdict lookups plus 371 homographs) is the stopgap if neither BART weight set can be used in WP4's window. It is third choice because its neural model's training data is undocumented, its ARPAbet output needs the mapping of 2.4, and it has not changed since 2019.

## 3. The speaker check bar

### 3.1 What the bar demands, in numbers

The bar's statistic is the one-sided 95 percent Clopper-Pearson upper bound. Computed exactly [ran 10-21]:

| Errors allowed | Smallest number of trials for an upper bound under 1 percent |
|---|---|
| 0 | 299 |
| 1 | 473 |
| 2 | 628 |
| 3 | 773 |
| 4 | 913 |
| 5 | 1,049 |

BUILD.md's "zero misses in 300 just reaches and one miss needs about 500" is right. The chance of passing one side, given the true error rate on that side [ran 10-21]:

| True rate | 300 trials (0 errors allowed) | 500 trials (1 allowed) | 1,000 trials (4 allowed) | 2,000 trials (12 allowed) |
|---|---|---|---|---|
| 0.1 percent | 0.74 | 0.91 | 1.00 | 1.00 |
| 0.2 percent | 0.55 | 0.74 | 0.95 | 1.00 |
| 0.3 percent | 0.41 | 0.56 | 0.82 | 0.99 |
| 0.5 percent | 0.22 | 0.29 | 0.44 | 0.79 |
| 1.0 percent | 0.05 | 0.04 | 0.03 | 0.04 |

Both sides must pass at one threshold, so the chances multiply. At a true 0.3 percent on both sides, 300 trials each pass with probability 0.41 × 0.41 = 0.17, and 1,000 each with 0.67. At 0.5 percent the figures are 0.05 and 0.19. In practice the bar asks for an operating point where both rates are around 0.1 to 0.3 percent, which means an equal error rate well under that, on 1 to 3 s of speech, pooled over the four room conditions.

### 3.2 What published models reach

Equal error rates on VoxCeleb1 with full-length trials, for models that run on a laptop CPU [primary]:

| Model | Licence | Parameters | ONNX in sherpa-onnx's speaker release | Vox1-O EER, percent | Harder sets, percent |
|---|---|---|---|---|---|
| WeSpeaker CAM++ (the one BUILD.md pins) | CC-BY-4.0: WeSpeaker says a pretrained model "follows the license of it's corresponding dataset", and names CC-BY-4.0 for VoxCeleb | 7.18M | `wespeaker_en_voxceleb_CAM++.onnx`, 29,292,684 B | 0.803; 0.718 with AS-norm; 0.707 after large-margin fine-tuning (the `_LM` file, 29,292,687 B) | Vox1-H 1.860 |
| WeSpeaker ResNet34 | CC-BY-4.0, as above (the Hugging Face copy is tagged Apache-2.0) | 6.63M | `wespeaker_en_voxceleb_resnet34.onnx`, 26,534,365 B | 0.867; 0.787 with AS-norm | Vox1-H 1.959 |
| 3D-Speaker CAM++, VoxCeleb | Apache-2.0 (ModelScope card) | 7.2M | `3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx`, 29,596,978 B | 0.73 on its card (0.65 in 3D-Speaker's README table) | Vox1-H 1.76 |
| 3D-Speaker CAM++, Chinese and English "advanced" | Apache-2.0 (ModelScope card) | 7.2M | `3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx`, 28,281,164 B | 1.16 | CN-Celeb 5.98; trained on large Chinese and English sets its card does not name |
| 3D-Speaker ERes2Net base, VoxCeleb | Apache-2.0 (ModelScope card) | 6.61M | `3dspeaker_speech_eres2net_sv_en_voxceleb_16k.onnx`, 26,485,263 B | 0.83 on its card (0.84 in the README) | weak on short tests (below) |
| 3D-Speaker ERes2NetV2 | Apache-2.0 (3D-Speaker repository) | 17.8M | Chinese 200k-speaker model only, 71,441,526 B; no English export | 0.61 after large-margin fine-tuning, 0.68 without | |
| SpeechBrain ECAPA-TDNN | Apache-2.0 | not stated on its card (3D-Speaker lists its own ECAPA-TDNN at 20.8M) | none; needs an export | 0.80 | |
| NVIDIA TitaNet-Large | CC-BY-4.0 | about 23M | `nemo_en_titanet_large.onnx`, 101,405,493 B (`nemo_en_titanet_small.onnx`, 40,257,283 B) | 0.66 | |
| ReDimNet-B2 (ID R&D) | MIT code; weight licence not stated | 4.7M | none | 0.57 after large-margin fine-tuning | the larger M model (trained partly on VoxBlink2): VOiCES 2.70, CN-Celeb 7.47 |
| ReDimNet2 B2, B4 and B6 (2026), VoxCeleb2-trained | MIT code; weight licence not stated | 3.6M, 6.6M and 12.3M | none | 0.57, 0.37 and 0.29 | Vox1-H 1.41, 1.07 and 0.99 |

**Cost on a CPU.** On one CPU thread, CAM++ runs at a real-time factor of 0.013, against 0.033 for a 14.66M-parameter ECAPA-TDNN and 0.032 for ResNet34 (CAM++ paper, Table 3) [primary]. At those rates a 3 s turn costs about 40 ms with CAM++ and about 100 ms with ResNet34 on one thread [inferred]; WP4 measures the real figures on this PC.

**Newer and stronger models.** Two kinds do not fit. Systems built on w2v-BERT 2.0 reach 0.12 percent on Vox1-O, but on a speech encoder of about 600M parameters, and pruned by 80 percent they are still near 120M [primary]. The VoxBlink2 models reach 0.228 percent (0.17 with score normalisation and quality calibration), but their licence is CC BY-NC-SA 4.0 and "no commercial application is allowed"; ECAPA2's weights are CC-BY-NC-4.0 [primary]. Any model trained partly on VoxBlink2, such as ReDimNet's M and S checkpoints, inherits that question [inferred]. One kind does fit: ReDimNet2 (arXiv, March 2026, submitted to Interspeech 2026) has VoxCeleb2-only checkpoints of 3.6M to 12.3M parameters that beat CAM++ on every VoxCeleb list [primary]. It has no ONNX export, no stated weight licence and no published short-test numbers, so it is a candidate for WP4's measurement once those are settled, not a reason to expect a pass: section 3.3's argument rests on short tests and room conditions, where every architecture above loses a factor of two to five. Two 2026 methods aimed at short speech publish no weights: DAME (ICASSP 2026) links no code, and VAM-ECAPA publishes code only and sits on WavLM.

**Short tests.** Results depend heavily on the protocol: whether enrollment stays full length while only the test is cut [primary]:

| System | Protocol | Full | 3 s | 2 s | 1.5 s | 1 s | Source |
|---|---|---|---|---|---|---|---|
| ResNet34 | full enrollment, test cut | | 1.47 | 2.12 | | | ERes2NetV2 paper, Table 3 |
| ECAPA-TDNN | same | | 1.27 | 1.95 | | | same |
| ERes2Net (base) | same | 0.84 | 1.89 | 3.28 | | | same |
| ERes2NetV2 | same | 0.61 | 0.98 | 1.48 | | | same |
| ECAPA-TDNN | test cut; enrollment not stated | 0.95 | | 1.79 | | 3.94 | MR-RawNet, Table 1 (5 s: 0.98) |
| ECAPA-TDNN | average of full enrollment with a cut test and the reverse | 1.03 | | 1.76 | 2.12 | 3.04 | GIST 2026, Table 2 |
| Best WavLM-based system there | same | 0.78 | | 1.24 | 1.46 | 2.29 | same |
| ResNet34, ECAPA-TDNN, ERes2NetV2 | 5 s enrollment, 1 s test | 1.02, 1.08, 0.91 | | | | 4.33, 4.84, 3.89 | DAME, Table 2 (with DAME: 3.29, 3.66, 3.26) |

DAME's 5 s enrollment and 1 s test is the closest published proxy for a short command. Where the protocol only says "standard", 1 s figures can be far worse: VAM-ECAPA's WavLM-based baseline goes from 2.393 percent at 3 s to 18.437 percent at 1 s, and VAM-ECAPA itself reaches 8.334 percent [primary]. The best published text-independent result on short tests that we found is ID R&D's system for the SdSV 2021 challenge. Enrollment there was 4 to 180 s of net speech and tests 1 to 8 s, and a fusion of nine ResNets trained for that data reached 0.69 percent EER [primary].

**Noise, distance and other talkers.** Even with full-length tests, the VOiCES far-field set costs 2.70 to 4.12 percent EER for ReDimNet's M and S models, and the multi-genre CN-Celeb 5.98 to 9.59 percent for the models above [primary]. With three interfering speakers mixed into each VoxCeleb trial, a baseline went from 0.88 to 12.84 percent EER; the target talker overlapped others 60.53 percent of the time, which the authors call higher than in real conversation, and their guided method brought it to 4.04 percent [primary].

### 3.3 Is the bar plausible on short commands?

**Not with the four conditions pooled, for any model that fits this PC.** On clean VoxCeleb with full-length enrollment, a 2 s test puts laptop models at 1.5 to 2.1 percent EER and a 1 s test at about 3 to 5 percent. Section 3.1 needs about a tenth of that. The owner's case is easier in some ways and harder in others [inferred]:

- **Easier:** one microphone and one room, a long enrollment, a few known other voices rather than thousands, and the owner's own consistent way of speaking to the PC.
- **Harder:** across the room is a far-field condition, where the published far-field figures are 2.7 to 4.1 percent EER even with long speech. Music or video through the speakers leaves residue after echo cancellation. "Another person talking in the room" is overlapped speech, where errors rose by more than an order of magnitude in the one published test above.

A single bad condition sinks the pooled bar, and the across-room and overlapping-speech conditions are each likely to exceed 1 percent on their own. A pass in the quiet desk condition alone is conceivable, but that is not what the bar measures, and the owner direction asks for real room conditions. **The expected result of WP4 acceptance 13 is `passed = false` with the numbers.** BUILD.md already makes that result harmless: the gate is inert, every rule that applies "when the speaker gate is on" does nothing, and the protections that never depended on voice stay (2.8).

Two consequences for the owner:

- Below the bar, an approval spoken by another voice inside the voice window is accepted (BUILD.md section 7 already lists this as the owner's choice). The research gives no reason to expect that to change in v0.1.
- The most dangerous impostor is Eileen itself. Eileen clones the owner's voice (Pocket TTS, Chatterbox), and speaker embedders accept synthetic speech readily. One 2019 study's TTS spoofs were accepted 54.69 percent of the time by an i-vector system and 68.34 percent by a GE2E system (the latter weakly trained), and the ASVspoof 5 organisers write that attacks "significantly compromise the baseline systems" [primary]. A speaker check would not stop Eileen's own clone. What does is echo cancellation against everything the PC plays and "no approval from audio Eileen itself started", which is the right design whatever the bar's outcome.

### 3.4 What WP4 needs: enrollment and test design

**Enrollment.**

- The first-run clip (about 30 s, one session, at the desk) counts as one enrollment utterance in practice, and the number of utterances matters a great deal. On CN-Celeb, an ECAPA-TDNN system with a PLDA back end improved from 18.44 percent EER with one enrollment utterance to 10.11 percent with five or more. A GE2E system on LibriSpeech's clean test set went from 3.92 to 2.01 percent as enrollment grew from 2 to 15 utterances; on the noisier test set it stopped improving after about four (3.59 to 2.35 percent) [primary]. Variety matters as much as count.
- For the measurement, enroll from at least three sessions on different days, at least two minutes of net speech in total, covering the desk, across the room, and one session while the PC plays audio [inferred]. Keep embeddings only; the audio stays on the PC and is never committed (owner answer 11).
- Compare one averaged centroid with one centroid per condition (score against the best match), and report both [inferred].
- If the gate ever enters shadow, embeddings from turns confirmed as the user's can be added to the enrollment, with a cap and an age limit. That changes the false-accept side too, so WP4 measures it before relying on it [inferred].

**Scoring.**

- Score the whole addressed turn after voice activity detection, not a fixed window. Record each trial's net speech duration and report rates in bins (under 1.5 s, 1.5 to 3 s, over 3 s) as well as pooled.
- Prefer models without large-margin fine-tuning for commands: WeSpeaker says its `_LM` models "could perform better on long audios, e.g. >3s" [primary]. The pinned CAM++ file is already the plain one.
- Try score normalisation (AS-norm) against a cohort of embeddings computed once from a permissively licensed corpus such as LibriSpeech (CC-BY-4.0), stored as embeddings only. WeSpeaker's tables show AS-norm alone is worth about 10 percent relative (CAM++ 0.803 to 0.718) [primary]. Also try a duration-aware threshold and a fusion of two embedders (CAM++ with ResNet34 or TitaNet); fusion is untested here [inferred].

**Test design** (refinements to acceptance 13; none weakens it):

1. **Fix the sizes and the allowed errors before scoring**, for example 500 owner and 500 other-voice test trials with at most one error each. Stopping when the numbers look good inflates the pass rate.
2. **Split by session, not by utterance.** Choose the threshold on one set of days and report the rates on another. A random split puts near-identical takes on both sides.
3. **More than five other voices where possible.** Trials from one person are correlated, so 300 trials from five people carry much less information than the bound assumes. Report each person's false-accept count. Include the people the owner lives or works with, who are the realistic impostors, and make at least half of the others the owner's sex with a similar accent. The bound itself stays as BUILD.md defines it.
4. **Define the four conditions precisely** (distance, playback level and content, the other talker's level and position) and balance trials across them. Report each condition and the pool.
5. **Run a pilot on the hardest condition first:** about 100 owner and 100 other-voice utterances across the room with another person talking. If the best threshold on the pilot already shows an error rate over 1 percent, the pooled bar cannot pass. Record `passed = false` with the pilot's numbers, and spare the owner the full recording session.
6. **Report synthetic and replayed voices separately.** Acceptance 13 already scores Kokoro's stock voices and a clone of someone else. Add a clone of the owner made by Eileen, both through the speakers and fed directly, and a recording of the owner replayed through the speakers. None of them counts toward the bar; they test the echo-cancellation and "audio Eileen started" protections.
7. **Approval words are a fixed set** ("yes, push", "approve"). A text-dependent check enrolled on those phrases is a separate and likelier case than open commands (SdSV's text-dependent task enrolls on three repetitions of a phrase [primary]). It is an idea for a later version, not a change to v0.1's bar.

**Cost to the owner.** At about 2 s per utterance plus prompting, 500 owner test utterances are roughly 30 to 40 minutes of recording across sessions, and each other person about 10 minutes [inferred]. The pilot in item 5 costs about 10 minutes and decides whether the rest is worth doing.

**Models to measure.** All of these are ONNX files already in sherpa-onnx's speaker release and usable through its speaker embedding API (its Rust examples include `speaker_embedding_extractor.rs` and `speaker_embedding_manager.rs`) [primary]: WeSpeaker CAM++ (the pinned baseline), WeSpeaker ResNet34 (the plain file, not `_LM`), 3D-Speaker CAM++ VoxCeleb, and TitaNet-Small and TitaNet-Large. Leave out 3D-Speaker's ERes2Net base: its 2 s result is the worst in the short-test table. ERes2NetV2 and ReDimNet2 have the best published numbers among small models, but both need an English ONNX export first, and ReDimNet2 needs a weight licence.

## 4. Proposed changes to BUILD.md (not made in this pull request)

This note changes no plan text. If the owner agrees, an amendment would:

1. **2.8 "No GPL in the speech build" and WP4's build script.** Note sherpa-onnx #4055: once 2.0.0 is released and 14 days old, pin its upstream prebuilt, keep `native.yml`'s check for espeak-ng symbols and data on that archive, and drop the from-source build if the check passes. Note that the GPL-free TTS path takes pre-phonemized input, so Pocket TTS keeps working (it needs no phonemizer) and Kokoro stays on `ort`. When downloading `kokoro-multi-lang-v1_0`, keep only the files Eileen uses and leave out `espeak-ng-data` and the lexicon files (they contain silver entries).
2. **2.8 and 4.2, the Kokoro front end.** In place of "a CMUdict lexicon plus a small ONNX G2P model, mapped to Kokoro's phoneme set", write: Eileen's names list, the user lexicon, misaki's gold lexicon (Apache-2.0) with its part-of-speech entries and rules, CMUdict mapped into Kokoro's alphabet, and misaki's BART model (Apache-2.0, 3,011,692 B, pinned revision) for unknown words, with Eileen's retrained weights as the fallback. Number reading is Eileen's own code, not a port of LGPL `num2words`. The `models.toml` row changes to match, with the fallback weights as a second row. CMUdict's licence is BSD-style with two conditions and an acknowledgement paragraph; BUILD.md calls it BSD-2-Clause, which the licence listing should state exactly.
3. **4.2, the CAM++ row.** The licence is CC-BY-4.0 (WeSpeaker's `docs/pretrained.md`), in place of "per WeSpeaker repository, WP4 verifies"; it needs an attribution line in NOTICE.
4. **WP4 acceptance.** Add the four G2P measurements of 2.5. Add the test-design refinements of 3.4 to acceptance 13: fixed sizes, a session split, per-person reporting, the pilot gate, duration bins, a clone of the owner and replayed audio reported, models without large-margin tuning.
5. **Section 7.** Restate the G2P risk with the measured ceiling on unknown words, and say plainly that the speaker check is expected to fail its bar on short commands.

## 5. Questions for the owner

1. **May espeak-derived data ship?** Two things are affected. misaki's silver lexicon is espeak-ng 1.52 output after transforms, entry by entry. The published BART weights were trained on gold plus 90 percent of silver. The FSF's FAQ says program output is not, in general, covered by the program's licence, but some silver entries reproduce espeak-ng's own exception lists, so a cautious reading treats the lexicon as derived from GPL data. The weights hold no entries verbatim. **Recommendation: leave the silver lexicon out of releases and ship the published BART weights**, with their training data stated in NOTICE. If the owner wants no espeak-derived input anywhere, the retrained fallback (2.6) becomes the primary at the cost of one training run.
2. **Is misaki's repository licence enough for the gold lexicon?** The repository is Apache-2.0, and sherpa-onnx and loqui already redistribute the lexicons under it. The sources behind gold are not named, and issue #107 asking for them has no reply. **Recommendation: accept the Apache-2.0 licence with attribution in NOTICE**, and keep the CMUdict-only retrain ready in case an answer changes the picture.
3. **Will more than five other people record for the speaker test?** Ten or more, including the people the owner lives or works with, would make the false-accept side mean something. If the pilot in 3.4 fails, nobody needs to record beyond it.

## 6. What was not done

- No model was downloaded, loaded or run, and no audio was recorded. Every accuracy and speed figure is published or third party, except the lexicon lookups, the parameter count and the binomial bounds.
- The lexicon counts and lookups were taken on 2026-10-21 from the data files at the commits named in 2.3. A review pass the same day recomputed the binomial bounds and re-read CMUdict's "eileen" entry in the file, but did not repeat every count.
- loqui's numbers come from a repository created on 2026-09-30, with no outside review and no stars; they are used as indications, not as facts.
- No accuracy figure for the BART model exists from its author. The one independent figure is on names only.
- The ARPAbet mapping in 2.4 is a proposal until WP4 measures its agreement with gold.
- Speaker verification on the owner's own room, microphone and voice is unmeasured by anyone. Section 3.3's judgement rests on public benchmarks, which differ from that case in the ways it lists.

## Sources

Opened on 2026-10-21.

Why GPL is avoided:

- Owner answer 12: [OWNER-DIRECTION.md](../OWNER-DIRECTION.md), 2026-10-20; Eileen's licence: the repository's `LICENSE` (MIT)
- espeak-ng licence: https://github.com/espeak-ng/espeak-ng (README, "License Information")
- FSF GPL FAQ, "If a library is released under the GPL ..." and "In what cases is the output of a GPL program covered by the GPL too?": https://www.gnu.org/licenses/gpl-faq.html#IfLibraryIsGPL and https://www.gnu.org/licenses/gpl-faq.html#WhatCaseIsOutputGPL

G2P and Kokoro:

- misaki: https://github.com/hexgrad/misaki (Apache-2.0); `misaki/en.py`, `pyproject.toml` and `EN_PHONES.md` on main; data commits: https://github.com/hexgrad/misaki/commits/main/misaki/data; pull request #74 (the BART fallback, merged 2025-04-14): https://github.com/hexgrad/misaki/pull/74; issue #51 (lexicon sources, answered 2025-02-14): https://github.com/hexgrad/misaki/issues/51; issue #105: https://github.com/hexgrad/misaki/issues/105; issue #107: https://github.com/hexgrad/misaki/issues/107; PyPI release history: https://pypi.org/project/misaki/
- BART G2P models: https://huggingface.co/PeterReid/graphemes_to_phonemes_en_us (files, `config.json`, `english_to_phonemes.py`, commits `823960d` and `746f1a6`) and https://huggingface.co/PeterReid/graphemes_to_phonemes_en_gb; community ONNX export: https://huggingface.co/konecth/graphemes_to_phonemes_en_us_onnx
- Kokoro: https://github.com/hexgrad/kokoro (`kokoro/pipeline.py`, `pyproject.toml`); https://huggingface.co/hexgrad/Kokoro-82M (model card and `config.json`)
- sherpa-onnx: pull request #4055, https://github.com/k2-fsa/sherpa-onnx/pull/4055; issue #3731, https://github.com/k2-fsa/sherpa-onnx/issues/3731; `sherpa-onnx/csrc/offline-tts-kokoro-impl.h` on master; `sherpa-onnx/csrc/kokoro-multi-lang-lexicon.cc` at v1.13.8; `scripts/kokoro/v1.0/generate_lexicon_en.py`; `rust-api-examples/examples/`; Kokoro package contents: https://k2-fsa.github.io/sherpa/onnx/tts/pretrained_models/kokoro.html; crate versions: https://crates.io/crates/sherpa-onnx
- num2words licence: https://pypi.org/project/num2words/
- CMUdict: https://github.com/cmusphinx/cmudict (`cmudict.dict`, `LICENSE`, commit history)
- OpenPhonemizer: https://github.com/NeuralVox/OpenPhonemizer; dataset: https://huggingface.co/datasets/mrfakename/ipa-phonemes-word-pairs
- DeepPhonemizer: https://github.com/as-ideas/DeepPhonemizer; https://pypi.org/project/deep-phonemizer/
- CharsiuG2P: https://github.com/lingjzhu/CharsiuG2P (README and `multilingual_results/multilingual/byt5_8_layers_baseline_100`, `byt5_16_layers_baseline_100`, `byt5_small_100`); checkpoint: https://huggingface.co/charsiu/g2p_multilingual_byT5_tiny_8_layers_100
- g2p_en: https://github.com/Kyubyong/g2p (`g2p_en/` file listing, `homographs.en`, commit history)
- SoundChoice: https://arxiv.org/abs/2207.13703 (Tables 4 and 6); model: https://huggingface.co/speechbrain/soundchoice-g2p
- Phonetisaurus: https://github.com/AdolfVonKleist/Phonetisaurus; Rust: https://crates.io/crates/phonetisaurus-g2p
- gruut: https://github.com/rhasspy/gruut
- Word-level G2P ceiling: Yolchuyeva, Németh and Gyires-Tóth, "Transformer based Grapheme-to-Phoneme Conversion", https://arxiv.org/abs/2004.06338 (Tables 2 and 3)
- loqui: https://github.com/Awakened-Labs/loqui (`NOTICE`, `docs/STATUS.md`, `crates/loqui-g2p/README.md`, `tools/parity/README.md`)
- Crates: https://crates.io/crates/loqui-g2p, https://crates.io/crates/misaki-rs (repository https://github.com/MicheleYin/misaki-rs), https://crates.io/crates/floravox-g2p and https://docs.rs/crate/floravox-g2p/latest, https://crates.io/crates/open-english-pronouncing-dictionary, https://crates.io/crates/voicegarden-lexicons and https://docs.rs/crate/voicegarden-lexicons/latest, https://crates.io/crates/sayd-misaki-en, https://crates.io/crates/espeak-ng, https://crates.io/crates/espeak-rs

Speaker verification:

- WeSpeaker pretrained models and licence: https://github.com/wenet-e2e/wespeaker/blob/master/docs/pretrained.md; VoxCeleb results: https://github.com/wenet-e2e/wespeaker/blob/master/examples/voxceleb/v2/README.md; Hugging Face copy: https://huggingface.co/Wespeaker/wespeaker-voxceleb-resnet34
- sherpa-onnx speaker models (asset list with sizes): https://github.com/k2-fsa/sherpa-onnx/releases/tag/speaker-recongition-models
- CAM++: https://arxiv.org/abs/2303.00332 (Tables 1 and 3)
- 3D-Speaker: https://github.com/modelscope/3D-Speaker (README tables; `egs/voxceleb/sv-eres2netv2`); ModelScope cards `iic/speech_campplus_sv_en_voxceleb_16k`, `iic/speech_campplus_sv_zh_en_16k-common_advanced`, `iic/speech_eres2net_sv_en_voxceleb_16k`
- ERes2NetV2: https://arxiv.org/abs/2406.02167 (Tables 1 and 3)
- MR-RawNet: https://arxiv.org/abs/2406.07103 (Table 1)
- Short-segment SV with pre-trained models (Gwangju Institute of Science and Technology; v2 2026-01-27): https://arxiv.org/abs/2509.19721 (Table 2)
- DAME: https://arxiv.org/abs/2601.13999 (Table 2)
- VAM-ECAPA, "Beyond Short Segments": https://arxiv.org/abs/2609.25007
- ReDimNet: https://github.com/IDRnD/redimnet (README, `EVALUATION.md`); ReDimNet2: https://arxiv.org/abs/2603.11841 and https://github.com/PalabraAI/redimnet2
- w2v-BERT 2.0 speaker verification: https://arxiv.org/abs/2510.04213
- TitaNet-Large: https://huggingface.co/nvidia/speakerverification_en_titanet_large
- SpeechBrain ECAPA: https://huggingface.co/speechbrain/spkrec-ecapa-voxceleb
- ECAPA2 licence: https://huggingface.co/Jenthe/ECAPA2
- VoxBlink2 terms and results: https://voxblink2.github.io/
- SdSV 2021 evaluation plan: https://arxiv.org/abs/1912.06311 (sections 2.1.3, 2.2.3 and 2.2.4); ID R&D system: https://www.isca-archive.org/interspeech_2021/alenin21_interspeech.html
- Overlapped speech: "Guided Speaker Embedding", https://arxiv.org/abs/2410.12182
- Multiple enrollment utterances: https://arxiv.org/abs/2209.00485 (Table IX) and https://arxiv.org/abs/2011.04896 (Table III)
- Spoofing: https://arxiv.org/abs/1910.13054 (Table 1); ASVspoof 5: https://arxiv.org/abs/2408.08739
- LibriSpeech licence: https://www.openslr.org/12

Related Eileen documents: [BUILD.md](../BUILD.md) (2.8, 4.2, 6.6 WP4, 7), [OWNER-DIRECTION.md](../OWNER-DIRECTION.md) (2026-10-20 answer 12, 2026-10-21), [2026-10-voice-stack.md](2026-10-voice-stack.md), [2026-10-dictation-and-voices.md](2026-10-dictation-and-voices.md).
