# Two voice risks: Kokoro without espeak-ng, and the speaker check bar

Research pass, 2026-10-21, on branch `docs/voice-risks`. Research only: web sources, repositories, model cards and papers. No model was downloaded or run, and no audio was recorded.

[BUILD.md](../BUILD.md) section 7 lists two open voice risks that this report looks at:

1. "The espeak-free speech build and Eileen's own Kokoro front end are new work; if the G2P model's quality is poor, Kokoro's pronunciation suffers." Section 2.8 ("No GPL in the speech build") plans a CMUdict lexicon plus a small permissively licensed ONNX grapheme-to-phoneme (G2P) model, mapped to Kokoro's phonemes and run through `ort`. Owner answer 12 rules out GPL in release binaries.
2. The speaker check bar (owner direction 2026-10-21; BUILD.md 2.8 and WP4 acceptance 13). A speaker check may gate anything only if, on the owner's consented command-length speech (one to three seconds) in four room conditions, the one-sided 95 percent Clopper-Pearson upper bounds of both the false-reject and the false-accept rate are under 1 percent, on at least 300 held-out owner utterances and 300 by at least five other people.

The report is for whoever builds `eileen-speech` in WP4, and for the owner, who has three questions to answer (section 5).

## How to read this

- **[primary]**: the project's own repository, source code, model card, release notes or paper, read on 2026-10-21.
- **[secondary]**: a third party's measurement. Most of these come from loqui, a small MIT-licensed Rust project published this month with no outside review yet, and are marked as such.
- **[ran 10-21]**: computed for this report. That means entry counts and word lookups in two pronunciation lexicons (plain data files, read with `python -I`), and the binomial bounds in section 3.1. No model was involved.
- **[inferred]**: our reasoning, not a measurement.

The PC is the one described in [2026-10-voice-stack.md](2026-10-voice-stack.md) (Core Ultra 7 155H, 16 GB). Other runs were building on it, and free memory read about 0.5 GB during this pass, which is one more reason nothing heavy was run.

## Verdict

**The G2P risk is smaller than BUILD.md assumed, and the plan should change in four places.**

1. **Kokoro's own G2P already has a GPL-free neural fallback, so Eileen should use it rather than invent one.** Kokoro's G2P library, misaki (Apache-2.0), gained a default fallback model for unknown words on 2025-04-14 (misaki pull request #74): a character-level BART model with 752k parameters (3.0 MB), Apache-2.0, trained on misaki's own lexicon. It writes Kokoro's phoneme alphabet directly, so no mapping is needed. It is on misaki's main branch only; the last PyPI release, 0.9.4 of 2025-04-05, still falls back to espeak-ng [primary]. One third-party test found it as good as espeak-ng on unknown names: 16.77 percent phoneme error rate (PER) against CMUdict on 500 names, where espeak-ng scored 17.46 percent [secondary, unreviewed].
2. **Upstream sherpa-onnx removed espeak-ng yesterday.** sherpa-onnx pull request #4055, merged on 2026-10-20, removes espeak-ng and piper-phonemize. The pull request says sherpa-onnx is "GPL-free after this PR", and the change is planned as release 2.0.0. The TTS generation config now accepts pre-phonemized input (`phoneme_codepoints` or `tokens`) in every binding, Rust included. In the lexicon path, a word that is not in the lexicon is now skipped [primary]. 2.0.0 is not released yet; crates.io's newest is 1.13.8 [primary]. Once 2.0.0 is out and past the two-week rule, the from-source build in `native.yml` is probably unnecessary (section 4).
3. **The lexicon should be misaki's gold dictionary plus CMUdict, not CMUdict alone.** misaki's `us_gold.json` has 90,201 entries, 790 of which change with the part of speech ("read": ɹˈid as default, ɹˈɛd as past tense), all in Kokoro's alphabet [ran 10-21]. The assistant's own name is not in it: "Eileen" is in neither misaki lexicon, while CMUdict (126,052 words, a BSD-style licence) has it as `AY0 L IY1 N`, and also has "GitHub" and "DaVinci" [ran 10-21]. So CMUdict belongs in a second layer, mapped into Kokoro's alphabet, between gold and the neural model. misaki's other lexicon, silver (93,361 entries), is espeak-ng 1.52 output run through transforms, in the maintainer's own words [primary]. Whether that may ship under "no GPL in releases" is an owner question (section 5, question 1).
4. **The ceiling on unknown words is set by English spelling, not by which model is chosen.** The best published word-level models get about one CMUdict word in five not exactly right (a 2M-parameter Transformer: 5.23 percent PER, 22.1 percent word error rate) [primary], and names are harder. Names Eileen will say often (the assistant's name, app names, voice names, contacts the user teaches) need a shipped names list and a user lexicon that "say it like ..." writes to.

**Recommendation.** Primary: a Rust port of misaki's English pipeline (tokenizer, a part-of-speech tagger, gold lexicon, suffix rules, numbers written from scratch), with a CMUdict layer and Eileen's own names list, and misaki's BART model for unknown words. Fallback: the same BART architecture retrained by Eileen on CMUdict mapped into Kokoro's alphabet, with a held-out test set. It uses the same runtime and the same code, and its data provenance is known by construction. Section 2.6 gives the reasons and the alternative.

**The speaker check bar is very unlikely to be met on short commands, and WP4 should plan for `passed = false`.** Published text-independent models small enough for this CPU reach 0.6 to 0.9 percent equal error rate (EER) on full-length VoxCeleb trials. With full-length enrollment and a 2 s test they are at 1.5 to 2.1 percent, and at 1 s at 3 to 4 percent [primary]. Far-field and noisy sets are at 2.7 to 8 percent even with long tests, and one interfering talker in the mixture takes a 0.88 percent system to 12.84 percent [primary]. The bar asks for much less than that. With 300 trials per side it allows no error at all. To pass with even even odds, the true rates must be near 0.2 percent on both sides at once, pooled over all four room conditions (section 3.1). A pass is conceivable in the quiet desk condition with long, multi-session enrollment. Across the room and with another person talking, the published evidence says no. BUILD.md already makes that outcome safe: with `passed = false` the gate is inert and nothing is refused for voice. Section 3.4 says what enrollment and test design WP4 needs so that the measurement is honest and cheap.

## 1. What Kokoro expects from a front end

### 1.1 The phoneme alphabet

Kokoro v1.0 was trained on misaki's phonemes, and its token vocabulary is misaki's alphabet. misaki's `EN_PHONES.md` lists 41 shared symbols: two stress marks (ˈ ˌ), 22 IPA consonants, the affricates ʤ and ʧ, ten IPA vowels, four capital-letter diphthongs (A for eɪ, I for aɪ, W for aʊ, Y for ɔɪ) and a custom ᵊ. American English adds æ, O (oʊ), ᵻ and the flap ɾ; British English adds a, Q, ɒ and ː [primary]. Stress marks sit immediately before the stressed vowel ("lead" is lˈid) [ran 10-21]. One phoneme chunk holds at most 510 tokens in Kokoro's pipeline [primary].

### 1.2 What Kokoro's own pipeline does

`KPipeline` in hexgrad/kokoro (Apache-2.0) builds `en.G2P(trf=trf, british=..., fallback=fallback, unk='')`. `fallback` is misaki's `EspeakFallback` when espeak-ng is installed; otherwise the pipeline logs "EspeakFallback not Enabled: OOD words will be skipped" [primary]. So Kokoro's reference pipeline either calls a GPL program for unknown words or drops them silently. Kokoro's other languages (Spanish, French, Hindi, Italian, Portuguese) go through espeak-ng entirely (`EspeakG2P`) [primary], so a Kokoro voice in those languages has no GPL-free front end today. That does not touch v0.1, which converses in English, but it matters for ROADMAP v0.2 (owner answer 21).

misaki's English G2P (`misaki/en.py`) [primary]:

1. spaCy's `en_core_web_sm` (MIT) tokenizes and tags, with only `tok2vec` and `tagger` enabled. `trf=True` swaps in `en_core_web_trf`.
2. Special cases for function words whose sound depends on the tag or the next word ("a", "the", "to", "used").
3. Lexicon lookup, gold before silver, choosing the entry for the Penn tag, then its parent tag, then `DEFAULT`. Title case and all-caps change stress.
4. Suffix rules: strip -s, -ed or -ing, look up the stem, and add the right ending back (z, s or ᵻz; t, d or ᵻd; flapping where the stem ends in a vowel and t).
5. Numbers, currencies, ordinals and years through `num2words`, and symbols (%, &, +, @) and dotted abbreviations through tables.
6. Anything still unknown goes to the fallback, and failing that to the `unk` marker (❓ by default).

The `misaki[en]` extra installs `num2words`, `spacy`, `phonemizer-fork`, `espeakng-loader`, `torch` and `transformers` [primary]. `num2words` is LGPL [primary] and `phonemizer-fork` wraps espeak-ng, so Eileen's number reading should be written from its own specification, not ported from `num2words` [inferred].

### 1.3 What sherpa-onnx does with Kokoro

The `kokoro-multi-lang-v1_0` package that BUILD.md 4.2 pins carries `lexicon-us-en.txt` and `lexicon-gb-en.txt`. sherpa-onnx's `generate_lexicon_en.py` builds them by merging misaki's silver and gold dictionaries (gold wins), keeping only each word's `DEFAULT` pronunciation (part-of-speech entries are dropped), and adding "Kokoro" and "Misaki" by hand [primary]. In 1.13.8, a word missing from the lexicon goes to espeak-ng. On master after pull request #4055, the Kokoro lexicon path logs "OOV word skipped" and moves on [primary]. The package also ships `espeak-ng-data` ([2026-10-voice-stack.md](2026-10-voice-stack.md), risk 3).

BUILD.md 2.8 already runs Kokoro through `ort` with Eileen's own front end, not through sherpa-onnx's TTS. So the sherpa-onnx change matters for the native build (section 4), not for how Kokoro speaks.

## 2. Grapheme-to-phoneme options without GPL code

### 2.1 The candidates

| Option | Licence (code / weights / data) | Size | Output | Published accuracy | CPU cost | Names and unknown words | Rust or ONNX readiness | Verdict |
|---|---|---|---|---|---|---|---|---|
| misaki's BART fallback (`PeterReid/graphemes_to_phonemes_en_us`, `_en_gb`) | Apache-2.0 (Hugging Face tag; the model card is empty) / trained on misaki's gold lexicon plus plurals | 752k parameters; `model.safetensors` 3,011,692 B; d_model 128, one encoder and one decoder layer, feed-forward 1,024, 64 positions | Kokoro's alphabet, with stress | None published. The training script in misaki #74 holds out no test set. Third party: 16.77 percent PER on 500 CMUdict names misaki lacks (espeak-ng 17.46), and a Rust port matches PyTorch on 500 of 500 words [secondary, unreviewed] | About 0.4M multiply-adds per output phoneme, so well under 1 ms per word [inferred] | One word at a time, no context, no confidence score; words over about 62 characters must be split | Plain Rust forward pass (loqui-g2p does it with `libm`), a community ONNX export (`konecth/graphemes_to_phonemes_en_us_onnx`, 2025-12-15), or Eileen's own pinned export | **Primary** |
| espeak-ng (misaki's `EspeakFallback`, phonemizer, piper-phonemize) | GPL-3.0-or-later | small | espeak IPA, mapped by misaki | The usual reference | fast | Rules plus exceptions | `espeak-ng` crate 0.2.0 is a Rust port, still GPL | Excluded (owner answer 12); useful only as a comparison in development |
| OpenPhonemizer (NeuralVox) | BSD-3-Clause-Clear / weights tagged the same / trained on `mrfakename/ipa-phonemes-word-pairs`, CC-BY-SA-4.0, about 275k pairs "generated using phonemizer/espeak" | DeepPhonemizer models; the dictionary variant is about 100 MB larger | espeak-style IPA | PhonemizerBench, scored as agreement with espeak (espeak = 100): 93.5, against g2p_en 86.1, DeepPhonemizer 85.0, gruut 74.8 | not given | DeepPhonemizer dictionary, then the model | Community ONNX exports (2026-01-02, 2026-09-29) | No: it imitates espeak by design, its data is share-alike and espeak-made, and the repository was archived on 2026-03-15 as no longer maintained |
| DeepPhonemizer (as-ideas) | MIT / CMUdict (ARPAbet or IPA) and WikiPron models | not stated | ARPAbet or IPA | None published beyond "comparable to state-of-art" | not given | Dictionary first, Transformer for unknown words, a confidence per word | TorchScript; no ONNX path documented; last release 0.0.19 on 2021-05-14 | Possible, but stale and needs an export and a mapping |
| CharsiuG2P | MIT code / model licence not stated / mixed sources, some espeak-made | ByT5 tiny (8 layers, 29 MB checkpoint) to small | IPA | Its own results for `eng-us`: PER 14.6, 12.6 and 10.0 percent, WER 47.0, 41.4 and 35.6 percent (tiny 8 layers, tiny 16 layers, small) | ByT5 tiny int8 about 64 ms per word, small about 150 ms and 1.2 GB (floravox's figures) [secondary] | Single words with a language prefix | No ONNX from the authors | No for English; worth a look for v0.2's other languages |
| g2p_en (Kyubyong) | Apache-2.0 / training data not documented | GRU seq2seq, `checkpoint20.npz` 3,342,298 B | ARPAbet | None published | numpy greedy decoding, well under 1 ms per word [inferred] | CMUdict, 371 part-of-speech homographs, GRU for the rest | Trivial to port; last change 2023-01-05 | Considered for the fallback (section 2.6) |
| SoundChoice (SpeechBrain) | Apache-2.0 / LibriG2P (CMUdict, LibriSpeech, Wikipedia homographs) | RNN with BERT word embeddings | ARPAbet | 2.65 percent PER on whole sentences, 94 percent homograph accuracy | needs a BERT model per sentence | Sentence-level, so homographs in context | PyTorch | No: too heavy for a live front end |
| Phonetisaurus | BSD-3-Clause / trains on any lexicon | WFST; about 70 MB per language in floravox's builds [secondary] | the lexicon's alphabet | Joint-sequence models of this family score about 5.9 percent PER and 24.5 percent WER on CMUdict | about 1 ms per word [secondary] | n-gram over grapheme-phoneme pairs | `phonetisaurus-g2p` crate (MIT, 0.1.1, 2025-04-08) | Considered; larger, and an older method |
| gruut | MIT | CRF | IPA | PhonemizerBench 74.8 | | | archived 2024-11 | No |

Sources for every row are listed at the end. "Joint-sequence" refers to the CMUdict results table of Yolchuyeva et al., which also gives the Transformer figures in the verdict [primary].

### 2.2 Rust crates published this year

None of these existed in a usable form when BUILD.md was written, and all are young and lightly used. Read their numbers as unreviewed.

| Crate | Licence | Newest release | What it is | Note |
|---|---|---|---|---|
| `loqui-g2p` (Awakened-Labs/loqui) | MIT | 0.4.0, 2026-10-20; first release 2026-10-01 | A full Rust port of misaki 0.9.4's English pipeline: spaCy's tokenizer rules as data, its own averaged-perceptron tagger (1.4 MB, trained on public-domain Gutenberg text labelled by `en_core_web_sm`), misaki's gold and silver lexicons, and both BART models embedded. Depends on `serde`, `serde_json`, `fancy-regex`, `unicode-normalization`, `libm` and `thiserror` | Its parity log against Python misaki 0.9.4 with espeak-ng: PER 0.01 percent on the 720 Harvard sentences, 0.72 percent on a held-out assistant set, 0.78 percent on 4,930 Gutenberg lines; tokenizer identical to spaCy on 1.83M tokens; numbers identical to `num2words` on 7,026 cases; tagger agreement with spaCy 96 percent. End to end, Kokoro speech transcribed by Whisper large-v3-turbo: 2.42 percent WER, against 2.28 percent for misaki with espeak-ng [secondary, unreviewed] |
| `misaki-rs` | MIT | 0.6.0, 2026-09-02 | A Rust port of misaki with an embedded tagger | The `espeak` feature is on by default. The README says the dictionary was extended "using eSpeak". Without espeak, unknown words are spelled letter by letter, and a `Fallback` trait lets a caller plug in a model. loqui's first cut on it read currency, units, ordinals and years wrongly: 11.52 percent PER on assistant text [secondary] |
| `floravox-g2p` (AACTools) | Apache-2.0 OR MIT | 0.8.8, 2026-10-14 | FST lexicons with pluggable fallbacks (Phonetisaurus, ByT5 ONNX, letter spelling); English data from misaki-rs | Pins `ort` =2.0.0-rc.14, newer than Eileen's =2.0.0-rc.13 |
| `open-english-pronouncing-dictionary` | CC-BY-SA-4.0 | 0.1.0, 2026-05-13 | A fused US English lexicon (misaki, CMUdict, WikiPron) of about 280k words | Share-alike data |
| `voicegarden-lexicons` (AACTools) | Apache-2.0 OR MIT code; MIT and BSD data | 0.3.0, 2026-09-01 | Fetches gruut-derived lexicons for 14 languages plus CMUdict | Offered on the sherpa-onnx issue as the espeak replacement for Piper voices |
| `sayd-misaki-en` | Apache-2.0 | 0.1.0, 2026-08-13 | "English G2P for Kokoro, ported from misaki" | No repository listed |

`loqui-g2p` is close to what WP4 has to build, and its parity method (Python misaki as the oracle, public-domain sentences, a held-out assistant set and a list of CMUdict words that misaki lacks) is the right test. Whether Eileen depends on it or writes its own port is WP4's call [inferred]. In favour of depending: it exists, and 0.3.0 (2026-10-04) is already past the two-week rule. Against: a 9.3 MB crate of embedded data from an organisation with no track record, `fancy-regex` in the hot path, and the spotless bar. Either way, it is the reference to measure against.

### 2.3 Names and unknown words

Lookups in misaki's main-branch lexicons and CMUdict's master file [ran 10-21]:

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
| lead | lˈid only (misaki issue #105: the metal sense is missing) | missing | two entries |

With misaki's lexicons alone, Eileen's own name goes to the neural fallback on every reply that says it [ran 10-21]. The same happens to many app names a user says, and to any name the user gives the assistant ("call you Nova now", E14). The word-level ceiling makes a better model the wrong lever. On CMUdict, a 2M-parameter Transformer gets 22.1 percent of held-out words wrong somewhere (5.23 percent PER); the joint-sequence model reaches 24.53 percent WER (5.88 percent PER) [primary]. Names are harder still: 16.77 percent PER for the BART model and 17.46 percent for espeak-ng on 500 names [secondary]. The levers are layers and learning [inferred]:

1. **User lexicon first.** `vocabulary` facts in the user model (BUILD.md 2.8 "Vocabulary") also carry a pronunciation. "Say Kokoro like co-co-ro", a spelled-out respelling or a pick from two candidates writes an entry, versioned and undoable like every setting.
2. **Eileen's shipped names list:** the assistant's default name and the names the user picks, the launch grammar's app names, Kokoro's voice names, and the product names Eileen says in its own phrases. Each entry is reviewed by ear once and kept in a small committed file.
3. **misaki gold**, with its part-of-speech entries.
4. **CMUdict mapped into Kokoro's alphabet** (section 2.4), for words gold lacks.
5. **misaki silver**, only if the owner allows espeak-derived data (section 5, question 1).
6. **misaki's suffix rules** on the stem.
7. **The BART model.**
8. **Letters spelled out** for all-capital tokens and anything the model cannot take (misaki's acronym rule).

A word that reaches layer 7 in a reply is also a candidate for the user lexicon. The HUD can offer "how do I say this?" once for a word Eileen keeps meeting [inferred].

### 2.4 Mapping CMUdict (and g2p_en) into Kokoro's alphabet

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

The other consonants are the same letter. Stress digits 1 and 2 become ˈ and ˌ placed just before the vowel. misaki's American set also has allophones CMUdict does not write. The flap ɾ is used for t or d between a vowel and an unstressed vowel ("water" is wˈɔɾəɹ in gold). The custom ᵊ marks a syllabic consonant ("gimble" comes out as ɡˈɪmbᵊl in misaki #74's examples). A handful of rules covers them. The check is cheap and needs no model. Map every CMUdict word that is also in gold and report the agreement, before and after the allophone rules. WP4 adopts the CMUdict layer only if the agreement is high (for example 95 percent of words exact after normalising stress), and the disagreements show what the rules miss [inferred].

### 2.5 How quality gets measured in WP4

All of these run in CI on text, with no recordings and no GPL code in anything shipped [inferred]:

1. **Parity:** Eileen's front end against Python misaki (main branch, with the BART fallback) on the 720 Harvard sentences (public domain) and an assistant-style set of numbers, money, times, dates, acronyms and homographs. The target is under 1 percent PER, normalised for stress and flaps, with every difference listed.
2. **Unknown words:** 500 or more CMUdict words absent from gold, mapped as in 2.4, with PER and WER for the BART model and the fallback; the same on a names set (first names, app and product names, place names).
3. **Eileen's own words:** every entry in the shipped names list round-trips exactly.
4. **Intelligibility:** Kokoro speaks the assistant set and the names set through Eileen's front end, Nemotron transcribes it, and WER is compared with the text. This finds the errors a listener would notice. It needs Kokoro and Nemotron loaded, so it runs on a CI runner or in the exclusive slot, never alongside a build on the owner's PC.

### 2.6 Recommendation: one primary and one fallback

**Primary: misaki's pipeline in Rust, with misaki's BART model for unknown words.** It is the front end Kokoro was trained against, so the alphabet, the stress placement and the allophones match without a mapping. The BART model is Apache-2.0, 3 MB and cheap enough to run on every unknown word inside the first clause's budget. It is misaki's own default since 2025-04-14, and the one independent measurement puts it level with espeak-ng on names [secondary]. It fits BUILD.md 2.8 as written ("a small permissively licensed ONNX grapheme-to-phoneme model for unknown words, mapped to Kokoro's phoneme set and run through `ort`"), except that no mapping is needed. WP4 either exports the encoder and one decoder step to ONNX once with a pinned script (as for AudioSeal), or runs the forward pass in plain Rust. The weights are the same pinned `model.safetensors` (revision `a5631b285d18d59483c32c0c3379cb9fac924f4b` for en_us) either way, and the choice is made by measurement.

**Fallback: the same BART architecture, retrained by Eileen on CMUdict mapped into Kokoro's alphabet.** It covers the two ways the primary can disappoint:

- **Quality.** If WP4's held-out test (2.5, item 2) shows the published model worse than wanted, a retrain on CMUdict's 126k words plus gold, with a proper held-out split, gives a measured number. CMUdict has many more names than gold, which is where the error concentrates.
- **Provenance.** misaki's maintainer says gold was "seeded" from "local datasets of IPA labels" and mapped with heuristics. Those sources are not named, and a request to name them (misaki issue #107, 2026-09-19) is still unanswered [primary]. A model trained only on CMUdict (BSD-style) has known data by construction.

It needs no new runtime code: same shape, same alphabet, same inference path, different weights. The training script is in misaki #74, and a 752k-parameter model trains in minutes on a GPU, for example on the owner's Colab with his go-ahead, or in a longer CI job [inferred]. g2p_en's GRU (Apache-2.0, CMUdict-based, no training needed) is the stopgap if the retrain cannot fit WP4's window. It is second choice because its training data is not documented and its ARPAbet output needs the mapping of 2.4.

## 3. The speaker check bar

### 3.1 What the bar demands, in numbers

The bar's statistic is the one-sided 95 percent Clopper-Pearson upper bound. Computed exactly [ran 10-21]:

| Errors allowed | Smallest number of trials for an upper bound under 1 percent |
|---|---|
| 0 | 299 |
| 1 | 473 |
| 2 | 628 |
| 3 | 773 |
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

Equal error rates on VoxCeleb1 (Vox1-O, cleaned) with full-length trials, for models that run on a laptop CPU through ONNX:

| Model | Licence | Parameters | ONNX in sherpa-onnx's release | Vox1-O EER, percent | Harder sets |
|---|---|---|---|---|---|
| WeSpeaker CAM++ (the one BUILD.md pins) | CC-BY-4.0: WeSpeaker says VoxCeleb-trained models follow VoxCeleb's licence | 7.18M | `wespeaker_en_voxceleb_CAM++.onnx`, 29,292,684 B | 0.803; 0.718 with AS-norm; 0.707 after large-margin tuning (the `_LM` file) | Vox1-H 1.86 |
| WeSpeaker ResNet34 | CC-BY-4.0, as above (the Hugging Face copy is tagged Apache-2.0) | 6.63M | `wespeaker_en_voxceleb_resnet34.onnx`, 26,534,365 B | 0.867; 0.787 with AS-norm | Vox1-H 1.96 |
| 3D-Speaker CAM++, VoxCeleb | Apache-2.0 (ModelScope) | 7.2M | `3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx`, 29,596,978 B | 0.65 | CN-Celeb 6.78 |
| 3D-Speaker CAM++, Chinese and English "advanced" | Apache-2.0 (ModelScope) | 7.2M | `3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx`, 28,281,164 B | not published | trained on large Chinese and English sets, per its card |
| 3D-Speaker ERes2NetV2 | Apache-2.0 (ModelScope) | 17.8M | Chinese 200k-speaker model only, 71,441,526 B; no English VoxCeleb export found | 0.61 | CN-Celeb 6.14 |
| SpeechBrain ECAPA-TDNN | Apache-2.0 | about 20M | none; needs an export | 0.80 | |
| NVIDIA TitaNet-Large | CC-BY-4.0 | about 23M | `nemo_en_titanet_large.onnx`, 101,405,493 B (`_small`, 40,257,283 B) | 0.66 | |
| ReDimNet-B2 (ID R&D) | MIT code; weight licence not stated | 4.7M | none | 0.57 after large-margin tuning | the larger M model: VOiCES 2.70, CN-Celeb 7.47 |

Cost on a CPU: CAM++ runs at a real-time factor of 0.013 on one thread, against 0.033 for ECAPA-TDNN and 0.032 for ResNet34 (CAM++ paper) [primary]. On one laptop core, loqui measured 8.4 ms per second of audio for CAM++ and 8.6 ms for ResNet34 [secondary]. A 3 s turn costs about 25 to 40 ms per model, and two models fused stay inside the end-of-turn window [inferred].

Newer and stronger models exist and do not fit. ReDimNet2 (Interspeech 2026) reaches 0.276; W2V-BERT 2.0 with adapters reaches 0.14 to 0.25, and WavLM-based systems 0.42 to 0.6, but they sit on speech encoders with hundreds of millions of parameters. The VoxBlink2-trained SimAM-ResNet100 reaches 0.202, but its licence is CC BY-NC-SA 4.0 ("no commercial application is allowed"), and ECAPA2's Hugging Face licence is CC-BY-NC-4.0 [primary]. Two 2026 methods aimed at short speech, DAME (ICASSP 2026) and VAM-ECAPA, have no released weights that we found.

**Short tests.** Results depend heavily on whether enrollment is full length and only the test is cut. With full-length enrollment and a cropped test [primary]:

| System | Full | 3 s | 2 s | 1.5 s | 1 s | Source |
|---|---|---|---|---|---|---|
| ResNet34 | | 1.47 | 2.12 | | | ERes2NetV2 paper (3D-Speaker reproduction) |
| ECAPA-TDNN | | 1.27 | 1.95 | | | same |
| ERes2NetV2 | 0.61 | 0.98 | 1.48 | | | same |
| ECAPA-TDNN | 0.95 | | 1.79 | | 3.94 | MR-RawNet paper (5 s: 0.98) |
| ECAPA-TDNN, 512 channels | 1.03 | | 1.76 | 2.12 | 3.04 | GIST 2026, enrollment and test cropped in turn |
| Best WavLM-based system there | 0.78 | | 1.24 | 1.46 | 2.29 | same |

When the protocol shortens both sides, or does not say, 1 s EER reaches 8 to 18 percent (VAM-ECAPA, 2026) [primary]. The best published text-independent short-test result is from the SdSV 2021 challenge. Enrollment there was 4 to 180 s, tests 1 to 8 s, and the winning entry reached 0.69 percent EER with a fusion of nine ResNets trained for that data [primary].

**Noise, distance and other talkers.** Even with full-length tests, the VOiCES far-field set costs 2.7 to 4.1 percent EER for ReDimNet's M and S models, and the multi-genre CN-Celeb 6 to 8 percent for every model above [primary]. With three interfering speakers mixed into VoxCeleb trials, a baseline went from 0.88 to 12.84 percent EER (guided speaker embedding paper, 2024) [primary].

### 3.3 Is the bar plausible on short commands?

**Not with the four conditions pooled, for any model that fits this PC.** On clean VoxCeleb with full-length enrollment, a 2 s test puts laptop models at 1.5 to 2.1 percent EER. Section 3.1 needs about a tenth of that. The owner's case is easier in some ways and harder in others [inferred]:

- **Easier:** one microphone and one room, a long enrollment, a few known other voices rather than thousands, and the owner's own consistent speaking style toward the PC.
- **Harder:** across the room is a far-field condition, where published EERs triple even with long speech. Music or video through the speakers, after echo cancellation, leaves residue. "Another person talking in the room" is overlapped speech, where errors rise by an order of magnitude.

A single bad condition sinks the pooled bar. The across-room and overlapping-speech conditions are each likely to exceed 1 percent on their own. A pass in the quiet desk condition alone is conceivable, but that is not what the bar measures, and the owner direction asks for real room conditions. The expected result of WP4 acceptance 13 is `passed = false` with the numbers. BUILD.md already makes that result harmless: the gate is inert, every rule that applies "when the speaker gate is on" does nothing, and the protections that never depended on voice stay (2.8).

Two consequences for the owner:

- Below the bar, an approval spoken by another voice inside the voice window is accepted (BUILD.md section 7 already lists this as the owner's choice). The research gives no reason to expect that to change in v0.1.
- The most dangerous impostor is Eileen itself. Eileen clones the owner's voice (Pocket TTS, Chatterbox), and speaker embedders accept synthetic speech readily. One study accepted 54.69 percent of TTS spoofs against an i-vector system and 68.34 percent against GE2E, and the ASVspoof 5 organisers report that attacks "significantly compromise" baseline systems [primary]. A speaker check would not stop Eileen's own clone. The protections that do are echo cancellation against everything the PC plays and "no approval from audio Eileen itself started", which is the right design whatever the bar's outcome.

### 3.4 What WP4 needs: enrollment and test design

**Enrollment.**

- The first-run clip (about 30 s, one session, at the desk) is one enrollment utterance in practice, and multiple utterances matter a great deal. On CN-Celeb, an ECAPA-TDNN system improved from 18.44 percent EER with one enrollment utterance to 10.11 percent with five or more. A GE2E system on LibriSpeech went from 3.92 to 2.01 percent as enrollment went from 2 to 15 utterances [primary].
- For the measurement, enroll from at least three sessions on different days, at least two minutes of net speech in total, covering the desk and across the room and one session while the PC plays audio [inferred]. Keep embeddings only; the audio stays on the PC and is never committed (owner answer 11).
- Compare one averaged centroid with one centroid per condition (score against the best match), and report both [inferred].
- If the gate ever enters shadow, embeddings from turns confirmed as the user's can be added to the enrollment, with a cap and an age limit. This changes the false-accept side too, so WP4 measures it before relying on it [inferred].

**Scoring.**

- Score the whole addressed turn after voice activity detection, not a fixed window. Record each trial's net speech duration and report rates in bins (under 1.5 s, 1.5 to 3 s, over 3 s) as well as pooled.
- Prefer models without large-margin fine-tuning for commands: WeSpeaker says its `_LM` models "could perform better on long audios, e.g. >3s" [primary]. The pinned CAM++ file is already the plain one.
- Try score normalisation (AS-norm) against a cohort of embeddings computed once from a permissively licensed corpus, such as LibriSpeech (CC-BY-4.0), stored as embeddings only. Also try a duration-aware threshold and a fusion of two embedders (CAM++ with ResNet34 or TitaNet). The WeSpeaker tables show AS-norm alone is worth about 10 percent relative [primary]; fusion is untested here [inferred].

**Test design** (refinements to acceptance 13; none weakens it):

1. **Fix the sizes and the allowed errors before scoring**, for example 500 owner and 500 other-voice test trials with at most one error each. Stopping when the numbers look good inflates the pass rate.
2. **Split by session, not by utterance.** Choose the threshold on one set of days and report the rates on another. A random split puts near-identical takes on both sides.
3. **More other voices than five where possible.** Trials from one person are correlated, so 300 trials from five people carry much less information than the bound assumes. Report each person's false-accept count. Include the people the owner lives or works with, who are the realistic impostors, and at least half of the owner's sex and a similar accent. The bound itself stays as BUILD.md defines it.
4. **Define the four conditions** precisely (distance, playback level and content, the other talker's level and position) and balance trials across them. Report each condition and the pool.
5. **Run a pilot on the hardest condition first:** about 100 owner and 100 other-voice utterances across the room with another person talking. If the best threshold on the pilot already shows an error rate over 1 percent, the pooled bar cannot pass. Record `passed = false` with the pilot's numbers, and spare the owner the full recording session.
6. **Report synthetic and replayed voices separately**, as BUILD.md says: Kokoro's stock voices; a clone of the owner made by Eileen, both through the speakers and fed directly; and a recording of the owner replayed through the speakers. They never count toward the bar. They check the echo-cancellation and "audio Eileen started" protections.
7. **Approval words are a fixed set** ("yes, push", "approve"). A text-dependent check, enrolled on those phrases, is a separate and likelier case than open commands. It is an idea for a later version, not a change to v0.1's bar.

**Cost to the owner.** At about 2 s per utterance plus prompting, 500 owner test utterances are roughly 30 to 40 minutes of recording across sessions, and each other person about 10 minutes [inferred]. The pilot in item 5 costs about 10 minutes and decides whether the rest is worth doing.

**Candidates for the measurement, all ONNX files already in sherpa-onnx's release and usable through its speaker embedding API:** WeSpeaker CAM++ (the baseline already pinned), WeSpeaker ResNet34, 3D-Speaker CAM++ VoxCeleb and its Chinese-English "advanced" model, and TitaNet-Small and TitaNet-Large. ERes2NetV2 has the best published short-test numbers among small models, but it needs an English export first [primary].

## 4. Proposed changes to BUILD.md (not made in this pull request)

This report changes no plan text. If the owner agrees, an amendment would:

1. **2.8 "No GPL in the speech build" and WP4's build script.** Note sherpa-onnx #4055: once 2.0.0 is released and 14 days old, pin its upstream prebuilt. Keep `native.yml`'s check for espeak-ng symbols and data on that archive, and drop the from-source build if the check passes. Note that the GPL-free TTS path takes pre-phonemized input, so Pocket TTS keeps working (it needs no phonemizer) and Kokoro stays on `ort`. When downloading `kokoro-multi-lang-v1_0`, keep only the files Eileen uses and leave out `espeak-ng-data`.
2. **2.8 and 4.2, the Kokoro front end.** In place of "a CMUdict lexicon plus a small ONNX G2P model, mapped to Kokoro's phoneme set", write: Eileen's names list, the user lexicon, misaki's gold lexicon (Apache-2.0) with its part-of-speech entries and rules, CMUdict mapped into Kokoro's alphabet, and misaki's BART model (Apache-2.0, 3,011,692 B, pinned revision) for unknown words. Number reading is Eileen's own code, not a port of LGPL `num2words`. The `models.toml` row changes to match, with the fallback model as a second row.
3. **WP4 acceptance.** Add the four G2P measurements of 2.5. Add the test-design refinements of 3.4 to acceptance 13: a fixed size, a session split, per-person reporting, the pilot gate, duration bins, synthetic and replayed voices reported, non-LM models.
4. **Section 7.** Restate the G2P risk with the measured ceiling on unknown words, and say plainly that the speaker check is expected to fail its bar on short commands.

## 5. Questions for the owner

1. **May espeak-derived pronunciation data ship?** misaki's silver lexicon is espeak-ng 1.52 output after transforms, and misaki-rs extends its dictionary "using eSpeak". The output of a GPL program is not generally covered by the GPL, but these entries reproduce espeak-ng's own rules and exception lists, so a cautious reading treats them as derived from GPL data. Recommendation: leave silver out of releases for now. Ship gold plus CMUdict, measure the coverage lost (2.5, items 1 and 4), and decide again with the numbers.
2. **Is misaki's repository licence enough for the gold lexicon?** The repository is Apache-2.0, and sherpa-onnx and loqui already redistribute the lexicons under it. The sources behind gold are not named, and issue #107 asking for them is open. Recommendation: accept the Apache-2.0 licence with attribution in NOTICE, and keep the CMUdict-trained fallback (2.6) ready in case an answer changes the picture.
3. **Will more than five other people record for the speaker test?** Ten or more, including the people the owner lives or works with, would make the false-accept side mean something. If the pilot in 3.4 fails, nobody needs to record beyond it.

## 6. What was not done

- No model was downloaded, loaded or run, and no audio was recorded. Every accuracy and speed figure is published or third party, except the lexicon lookups and the binomial bounds.
- loqui's numbers come from a repository published this month, with no outside review and no stars; they are used as indications, not as facts.
- No accuracy figure for the BART model exists from its author. The one independent figure is on names only.
- The ARPAbet mapping in 2.4 is a proposal until WP4 measures its agreement with gold.
- Speaker verification on the owner's own room, microphone and voice is unmeasured by anyone. Section 3.3's judgement rests on public benchmarks, which differ from his case in the ways it lists.

## Sources

Read on 2026-10-21.

G2P and Kokoro:

- misaki: https://github.com/hexgrad/misaki (Apache-2.0); `misaki/en.py` and `pyproject.toml` on main; `EN_PHONES.md`; pull request #74 (the BART fallback, merged 2025-04-14): https://github.com/hexgrad/misaki/pull/74; issue #51 (lexicon sources, answered 2025-02-14): https://github.com/hexgrad/misaki/issues/51; issue #107 (open): https://github.com/hexgrad/misaki/issues/107; issue #105: https://github.com/hexgrad/misaki/issues/105; PyPI release history: https://pypi.org/project/misaki/; lexicon data: https://github.com/hexgrad/misaki/tree/main/misaki/data
- BART G2P models: https://huggingface.co/PeterReid/graphemes_to_phonemes_en_us and https://huggingface.co/PeterReid/graphemes_to_phonemes_en_gb; community ONNX export: https://huggingface.co/konecth/graphemes_to_phonemes_en_us_onnx
- Kokoro pipeline: https://github.com/hexgrad/kokoro (`kokoro/pipeline.py`, `pyproject.toml`)
- sherpa-onnx: pull request #4055, https://github.com/k2-fsa/sherpa-onnx/pull/4055; issue #3731, https://github.com/k2-fsa/sherpa-onnx/issues/3731; `sherpa-onnx/c-api/c-api.h` and `sherpa-onnx/csrc/offline-tts-kokoro-impl.h` on master; `scripts/kokoro/v1.0/generate_lexicon_en.py` and `run.sh`; `rust-api-examples/examples/kokoro_tts_en_phonemize.rs`; crate versions: https://crates.io/crates/sherpa-onnx
- num2words licence: https://pypi.org/project/num2words/
- CMUdict: https://github.com/cmusphinx/cmudict (`cmudict.dict`, `LICENSE`)
- OpenPhonemizer: https://github.com/NeuralVox/OpenPhonemizer; dataset: https://huggingface.co/datasets/mrfakename/ipa-phonemes-word-pairs
- DeepPhonemizer: https://github.com/as-ideas/DeepPhonemizer; https://pypi.org/project/deep-phonemizer/
- CharsiuG2P: https://github.com/lingjzhu/CharsiuG2P (`multilingual_results/multilingual/`); paper arXiv 2204.03067
- g2p_en: https://github.com/Kyubyong/g2p
- SoundChoice: https://arxiv.org/abs/2207.13703; model: https://huggingface.co/speechbrain/soundchoice-g2p
- Phonetisaurus: https://github.com/AdolfVonKleist/Phonetisaurus; Rust: https://github.com/lastleon/phonetisaurus-g2p-rs
- gruut: https://github.com/rhasspy/gruut
- Word-level G2P ceiling: Yolchuyeva, Németh and Gyires-Tóth, "Transformer based Grapheme-to-Phoneme Conversion", https://arxiv.org/abs/2004.06338 (Tables 2 and 3)
- loqui: https://github.com/Awakened-Labs/loqui (`NOTICE`, `crates/loqui-g2p`, `tools/parity/README.md`)
- Crates: https://crates.io/crates/misaki-rs, https://crates.io/crates/loqui-g2p, https://crates.io/crates/floravox-g2p, https://docs.rs/crate/floravox-g2p/latest, https://crates.io/crates/open-english-pronouncing-dictionary, https://crates.io/crates/voicegarden-lexicons, https://crates.io/crates/sayd-misaki-en; misaki-rs: https://github.com/MicheleYin/misaki-rs

Speaker verification:

- WeSpeaker pretrained models and licence: https://github.com/wenet-e2e/wespeaker/blob/master/docs/pretrained.md; VoxCeleb results: https://github.com/wenet-e2e/wespeaker/blob/master/examples/voxceleb/v2/README.md
- sherpa-onnx speaker models: https://github.com/k2-fsa/sherpa-onnx/releases/tag/speaker-recongition-models
- CAM++: https://arxiv.org/abs/2303.00332
- 3D-Speaker: https://github.com/modelscope/3D-Speaker; ModelScope models `iic/speech_campplus_sv_en_voxceleb_16k`, `iic/speech_campplus_sv_zh_en_16k-common_advanced`, `iic/speech_eres2netv2_sv_zh-cn_16k-common`
- ERes2NetV2: https://arxiv.org/abs/2406.02167
- MR-RawNet: https://arxiv.org/abs/2406.07103
- Short-segment SV with pre-trained models (GIST, v2 2026-01-27): https://arxiv.org/abs/2509.19721
- VAM-ECAPA, "Beyond Short Segments": https://arxiv.org/abs/2609.25007
- DAME: https://arxiv.org/abs/2601.13999
- ReDimNet: https://github.com/IDRnD/redimnet (`EVALUATION.md`)
- TitaNet-Large: https://huggingface.co/nvidia/speakerverification_en_titanet_large
- SpeechBrain ECAPA: https://huggingface.co/speechbrain/spkrec-ecapa-voxceleb
- ECAPA2 licence: https://huggingface.co/Jenthe/ECAPA2
- VoxBlink2 terms: https://voxblink2.github.io/
- SdSV 2021 evaluation plan: https://arxiv.org/abs/1912.06311; winning text-independent system: https://www.isca-archive.org/interspeech_2021/alenin21_interspeech.html
- Overlapped speech: "Guided Speaker Embedding", https://arxiv.org/abs/2410.12182
- Multiple enrollment utterances: https://arxiv.org/abs/2209.00485 and https://arxiv.org/abs/2011.04896
- Spoofing: https://arxiv.org/abs/1910.13054; ASVspoof 5: https://arxiv.org/abs/2408.08739

Related Eileen documents: [BUILD.md](../BUILD.md) (2.8, 4.2, 6.6 WP4, 7), [OWNER-DIRECTION.md](../OWNER-DIRECTION.md) (2026-10-21), [2026-10-voice-stack.md](2026-10-voice-stack.md), [2026-10-dictation-and-voices.md](2026-10-dictation-and-voices.md).
