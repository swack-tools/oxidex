# Step 18 design checkpoint — `TagOccurrence` and `TagSink`

**Status: awaiting maintainer sign-off. No implementation has started.**
Orchestrator-authored (plan rule 7 — never delegated). Base: `refactor/tag-machinery`
@ `3d5a6a2a`. This is the resolution of disagreement **D1** and the largest single type
change in the plan (effort L).

Unlike the Step 10 and Step 15 artifacts, this file is **committed to the branch**, so
`git worktree add` carries it to the implementing agent. Both earlier checkpoints told an
agent to read a design file that its worktree had never received.

---

## 1. The problem, restated from evidence

`MetadataMap` (`src/core/metadata_map.rs`, 433 lines) is a `HashMap<String, TagValue>`
plus a private `value_forms: HashMap<String, String>` sidecar. `insert()` overwrites.

That single fact causes seven verified defects (Part V of the merged review):

| Finding | Consequence |
|---|---|
| tagmodel/1.2 | ~209–215 repeated `group:name` cases across 53/194 files; ~89–94 with **distinct values**, irreversibly lost |
| tagmodel/1.3 | Wrong default winners — no priority arbitration exists |
| tagmodel/1.4 | `-EXIF:Make` returns nothing; `-a` is a no-op; `-G*` is no-op'd in the CLI |
| tagmodel/1.5 | `--no-print-conv` cannot restore raw, because formatting happens *before* storage |
| tagmodel/1.6 | Family-1 `System` unrepresentable |
| composites/6.4 | Composite inputs resolved by hard-coded rank + suffix scan |
| jxl-cr3-warn/6b | QuickTime tracks flattened into invented `_N` names |

The Casio fix this session is the cheapest illustration: one wrong tag *name*
(`CCDSensitivity` where ExifTool says `ISO`) silently cost two tags, because
`Composite:LightValue` resolves its dependency by scanning for any `*:ISO` key. Suffix
scanning is not a bug to patch; it is the absence of a real occurrence model.

**Scale of the change:** 170 files reference `MetadataMap`; roughly 4,034 `insert(` call
sites and 2,743 `get`-family call sites. Any design requiring all of them to change at
once is not viable.

---

## 2. The design

### 2.1 `TagOccurrence` — the record

Modelled on ExifTool's `FoundTag` (`ExifTool.pm:9448`+), which is the contract we are
reproducing: it assigns a priority, records group families, and on a duplicate key mints
`"$tag ($nextInd)"` from `$$self{DUPL_TAG}` rather than overwriting.

```rust
pub struct TagOccurrence {
    pub id:        TagId,          // canonical numeric/table identity where one exists
    pub name:      &'static str,   // or interned String
    pub group0:    Group,          // family 0: EXIF, File, XMP, MakerNotes, Composite…
    pub group1:    Group,          // family 1: IFD0, Track3, System, ICC-header…
    pub group2:    Option<Group>,  // family 2 where known
    pub instance:  Instance,       // per-instance doc/track identity (Doc3, Track2)
    pub raw:       TagValue,       // pre-conversion
    pub value:     Option<TagValue>, // ValueConv form
    pub print:     Option<TagValue>, // PrintConv form
    pub priority:  u8,             // FoundTag's Priority / PRIORITY / Avoid
    pub is_list:   bool,
    pub order:     u32,            // file order — the tiebreak
    pub origin:    Provenance,     // module, table, byte range
}
```

Three forms, not one, is the load-bearing part: `--no-print-conv` (1.5) and the composite
layer (6.3/6.4) both need a form the current code destroys before storage.

### 2.2 `TagSink` — collection, and the winner projection

`TagSink` accumulates occurrences in file order. A **winner projection** reduces it to
today's `MetadataMap`, reproducing current output exactly:

1. highest `priority` wins;
2. tie → lowest `order` (first arrival), matching FoundTag;
3. the loser is retained, not dropped — reachable via `-a`.

`MetadataMap` survives **unchanged as the projected view**. That is what makes this
tractable at 4,034 call sites: parsers keep calling `insert()`, which becomes a
thin shim minting an occurrence with default priority and next order.

### 2.3 Migration shape — and the honest risk

This is the part I want your eye on, because it is where the effort estimate lives.

- **Phase A (this step):** add the types, the sink, the projection, and the `insert()`
  shim. Zero intended behavior change. Gate: full-corpus A/B byte-identical.
- **Phase B (Step 19):** migrate the exemplar families — JPEG COM duplicates, QuickTime
  tracks to family-1 `Track1..N`, filesystem tags to `System`, one MakerNote family.
- **Phase C (Steps 20–22):** output projection (`-a`, `-G*`, `-n`), request-awareness,
  composites on the winner view.

**The risk I will not paper over:** a shim that mints occurrences from `insert()` cannot
recover what the caller already threw away. Where a parser formats before storing, the
raw form is *already gone* at the shim boundary — so Phase A gives structure without
fidelity for those tags, and every one of them must be migrated by hand in Phase B/C to
actually populate `raw`/`value`. The plan's `rg 'format!' src/parsers` audit is the way to
size that; I have not run it yet and would rather scope it inside the step than guess now.

---

## 3. Decisions I need

**D1 — Storage shape.** `Vec<TagOccurrence>` + lazily built index, or an
`IndexMap<Key, SmallVec<[TagOccurrence; 1]>>`? Vec is simpler and preserves file order
natively; IndexMap makes lookup O(1) without a rebuild. **Recommendation: `Vec` plus a
lazily-built index**, because file order is semantically load-bearing (it is FoundTag's
tiebreak) and 194-file corpora are small; optimise later against a benchmark, not a guess.

**D2 — Interning.** `&'static str` names work for generated tables but not for XMP, whose
property names are discovered at parse time. **Recommendation: an interner
(`Arc<str>` or a string arena) from the start**, since retrofitting one through 170 files
later is worse than paying for it now.

**D3 — Scope of Phase A.** Does Phase A include *any* real duplicate retention, or is it
purely additive scaffolding with the projection proving byte-identity?
**Recommendation: purely additive.** The A/B byte-identity gate is only meaningful if
nothing is intended to change; mixing in real behavior change makes a failing diff
ambiguous.

**D4 — `value_forms` sidecar.** Step 8 added it tactically and marked it superseded by
this step. Retire it inside Phase A (fold into `TagOccurrence.value`), or leave it until
Step 22 consumes the occurrence view? **Recommendation: leave it until Step 22.** Removing
it in Phase A means touching the composite layer, which breaks the "purely additive"
property D3 relies on.
