# Step 15 decision gate — does the table-driven engine hold?

**Status: DECIDED by the maintainer 2026-08-11.**
- **Gate: PASSED at 69.5%. Option (A) — proceed with the table-driven engine as planned,
  ORACLE-FIRST.** `verify_exprs.py` lands before the compiler rollout, not alongside it.
  Steps 24 and 28 stay as designed. 69.5% is treated as a ceiling; the compiler's actual
  *verified* coverage is reported per bump as its own ledger artifact and is what drives
  whether Step 28's enablement widens.
- **Conditions: add bitmask (`$$self{Member} & 0xNN`) as a seventh closed shape in Step
  23**, taking closed condition coverage past 85%.

Orchestrator-authored (plan rule 7 — never delegated). Measured against the pinned 13.59
dump on `refactor/tag-machinery` @ `09c7e2ad`.

**Instrument:** `tools/exiftool-tables/dump_tables.pl` run against
`/tmp/oxidex-exiftool-cache/exiftool/lib` (ExifTool 13.59, capability-probed), classified
by a purpose-written census script. 152 modules OK, 1,512 tables, 33,096 tag entries
walked (variants counted individually).

**Sanity check on the instrument itself:** the census independently reproduces the plan's
figures — **1,529 distinct expressions across 6,993 uses**, exactly. Two separately
derived measurements agreeing is the reason to trust what follows.

---

## 1. The number

The gate: **≥60–70% of expression USES inside grammar+helpers ⇒ the table-driven Stage 5
holds; below ⇒ descope Steps 24 and 28 toward hand porting.**

Share of *uses*, not distinct expressions — a handful of expressions carry hundreds of
uses each, so distinct-expression coverage would flatter the answer badly (39.5% vs
72.9% at the same reading).

| Reading | Uses in grammar | Share | Distinct |
|---|---|---|---|
| **STRICT** — no `$self` at all | 4,231 / 6,993 | **60.5%** | 542 / 1,529 (35.4%) |
| **PLAN** — the grammar as the plan writes it, incl. its named helper registry | 4,857 / 6,993 | **69.5%** | 568 / 1,529 (37.1%) |
| **PLAN+** — plus five same-shape pure helpers | 5,097 / 6,993 | **72.9%** | 604 / 1,529 (39.5%) |

**PLAN is the reading that answers the gate, and it lands at 69.5% — inside the band, at
its top.** STRICT clears the 60% floor on its own.

Why PLAN is the honest reading rather than a thumb on the scale: the plan's grammar
explicitly names `ConvertDateTime`, `PrintExposureTime`, `PrintFNumber`, `GPS::ToDMS` and
the Decode-UCS2 family as a *helper registry*. `$self->ConvertDateTime($val)` (396 uses,
the single most common expression in ExifTool) contains `$self` only as the invocant —
it is a call to a named helper, not arbitrary interpreter-state access. My first pass
classified it as out-of-grammar and returned 57.8%, i.e. a *fail*; that was a classifier
bug, not a finding. The same pass also rejected `$val * 180 / 0x80000000` because its
arithmetic rule did not accept hex literals. Both are fixed above. I am flagging this
because the wrong answer here would have descoped the largest piece of the plan.

PLAN+ adds `ConvertDuration`, `ConvertUnixTime`, `PrintFraction`, `ConvertTimeSpan`,
`PrintLensID` — named explicitly so the claim is auditable rather than a fudge factor.

### Distribution

| Bucket | Uses | Share | Distinct |
|---|---|---|---|
| out: state or other-tag | 1,461 | 20.9% | 789 |
| in: `$val` arithmetic | 1,412 | 20.2% | 154 |
| in: string interpolation | 939 | 13.4% | 79 |
| in: ternary | 795 | 11.4% | 156 |
| out: other | 675 | 9.7% | 172 |
| in: helper with args | 626 | 9.0% | 26 |
| in: sprintf | 618 | 8.8% | 85 |
| in: builtin (abs/int/log/exp/sqrt/IsInt) | 199 | 2.8% | 47 |
| in: bare helper | 183 | 2.6% | 5 |
| in: `tr///` | 85 | 1.2% | 16 |

By slot: PrintConv 3,167 · ValueConv 2,663 · RawConv 1,163.

The shape is strongly favourable: the in-grammar buckets are *dense* (1,412 uses from 154
distinct arithmetic expressions), while the out-of-grammar residue is *sparse* (1,461 uses
across 789 distinct). Long-tail residue is exactly what "refuse and count" handles well —
each refusal is cheap and individually rare.

---

## 2. Conditions are a separate, better-shaped population

The 6,993 figure covers ValueConv/PrintConv/RawConv only. `Condition` is stored as a plain
string in the dump, so it is not in that count. Measured separately: **457 distinct
conditions across 1,140 uses.**

| Shape | Uses | Share |
|---|---|---|
| `$$self{Member} == / != n` | 336 | 29.5% |
| `$$self{Member} =~ /regex/` | 330 | 28.9% |
| other | 224 | 19.6% |
| conjunction of the above | 98 | 8.6% |
| bare `$$self{Member}` | 56 | 4.9% |
| `$$self{Member} eq / ne "str"` | 46 | 4.0% |
| `$$valPt =~ /…/` | 33 | 2.9% |
| `$format` / `$count` comparison | 17 | 1.5% |

**80.4% of condition uses fall into six closed shapes.** This is materially better than
the conversion picture and directly supports Step 23's plan (variant arrays with a vetted
`regex-lite` subset differentially tested against Perl). The 19.6% residue is mostly
multi-clause member logic and bitmask tests like `$$self{BitM} & 0x80` — the latter is a
trivial seventh shape that would push closed coverage past 85%, worth folding into Step 23.

---

## 3. What I read from this

**The table-driven bet holds, but at the top of the band rather than comfortably inside
it.** Three things make me more confident than the bare 69.5% suggests:

1. **Density.** The top 20 expressions alone cover roughly a third of all uses, and they
   are the easy ones — `$val / 100`, `"$val mm"`, `sprintf("%.1f",$val)`. Compiler effort
   is front-loaded and pays immediately.
2. **The residue is safe to refuse.** 789 distinct expressions carrying 1,461 uses means
   most refusals affect one or two tags. Under the omit-and-count contract that is a
   counted absence, not a wrong value.
3. **Conditions are in better shape than conversions** (80.4% closed), and Step 23 depends
   on that, not on the 69.5%.

The honest counterweight: 69.5% is *not* a comfortable margin. If the differential oracle
in Step 15's implementation shows that even the in-grammar expressions need per-expression
verification — which is the entire point of `verify_exprs.py` — the effective automation
rate will be lower than the census suggests, because "inside the grammar" is a claim about
*shape*, not about *proven equivalence*. The census says the compiler is worth building;
only the differential oracle can say it is correct.

---

## 4. The decision I need

**My recommendation: proceed with the table-driven engine (Steps 24 and 28 as planned),
with two conditions.**

1. Build `verify_exprs.py` (the differential Perl-vs-Rust oracle) **before** the compiler
   rollout, not alongside it. The census justifies the investment; only the oracle
   justifies trusting the output. Every translated expression executes against the pinned
   Perl over a probe input set, and anything that disagrees is refused and counted.
2. Treat 69.5% as the *ceiling*, not the target. Report the compiler's actual verified
   coverage per bump as its own ledger artifact, and let that number — not this one —
   drive whether Step 28's engine enablement widens.

The alternative, descoping toward hand porting, I do not recommend on this evidence: at
69.5% of uses, hand-porting means writing and maintaining by hand what a compiler would
absorb for roughly 154 arithmetic forms and 26 helper call sites, and the plan's own
history (the #636 regen finding 401 stale hand-embedded discrepancies with nothing able to
report them) is the argument against more hand code.

**Options:**

- **(A) Proceed as planned** — build the compiler, oracle-first. *(recommended)*
- **(B) Proceed, but narrower** — compile only the dense head (arithmetic, sprintf,
  interpolation, the five named helpers ≈ 55% of uses) and refuse ternaries and `tr///`
  initially, cutting grammar risk at the cost of ~14 points of coverage.
- **(C) Descope to hand porting** — Steps 24 and 28 become manual work queues driven by
  the triage report.

I will not start Step 15's implementation until you choose.
