# GSA Online UI / Design Architecture — Migration Notes for Future Local Replan

## Status

```text
PURPOSE = architecture input for future GSA Local full replan
AUTHORITY = NON-CANONICAL / READ-ONLY REFERENCE
SOURCE = current GPT-supper-agent plans + architecture docs
DO NOT = implement directly from this file
DO NOT = mutate current GSA Local plan from this file automatically
DO NOT = treat planned Online features as already deployed runtime capability
```

This note exists only to capture the architectural changes discovered in the newer GSA Online design so the later GSA Local total replan starts from the correct target model.

---

# 1. Important runtime distinction

The current GSA Online production runtime is V2 stable, but the newer UI Coder / Penpot / upgraded Tester architecture is still planned architecture rather than fully activated production behavior.

Observed Online baseline:

```text
V2 runtime = deployed / active
UI Coder Penpot upgrade = planned
Tester checkpoint upgrade = planned
EvidenceNeed / TestCheckpoint upgrade = planned
new UI workflow integration = planned
```

Therefore GSA Local should learn from the **target architecture**, but must not assume those Online capabilities already exist or are proven.

Any later Local plan must preserve capability honesty:

```text
planned architecture != observed runtime capability
```

---

# 2. Online target role model

The newer Online model is:

```text
Planner
Designer
Coder
  ├─ UX Coder
  └─ UI Coder
Reviewer
Tester
Orchestrator
```

Key boundaries:

## Planner

Owns:
- product intent;
- UX execution contract;
- functional flow/state semantics;
- topology;
- Task/Job boundaries;
- evidence requirements;
- review/test/Human Gate placement;
- acceptance/verification requirements;
- UI_FIRST vs UX_FIRST classification.

Planner does not own detailed visual methodology.

## Designer

Owns:
- visual interpretation;
- design coverage;
- visual system;
- responsive intent;
- artwork/icon language;
- asset requirements;
- implementation-ready design artifacts.

Designer is source read-only.

Designer does not own product behavior.

## UX Coder

Focuses on behavioral implementation:
- navigation/flow;
- state transitions;
- loading/empty/error/success;
- validation;
- keyboard/touch;
- accessibility behavior;
- responsive semantics;
- progression/recovery;
- data/state wiring.

## UI Coder

Focuses on visual implementation:
- composition;
- hierarchy;
- scale;
- spacing/density;
- typography;
- visual tokens;
- artwork/icon placement;
- blending;
- state appearance;
- responsive visual composition.

UI Coder owns source implementation HOW, not unresolved product/design decisions.

## Tester

Upgraded target role:
- independent checkpoint test planning;
- VERIFY / MEASURE / PROBE;
- practical browser/UI verification where capability is proven;
- screenshots/evidence;
- responsive/layout observations;
- runtime capability probing;
- evidence production for later Planner/Coder work.

Tester is not the visual decision-maker.

## Human

Retains authority for:
- material product decisions;
- architecture changes;
- subjective visual acceptance;
- merge/release authority.

---

# 3. Product workflow classification

Every UI-bearing Project is classified independently on two axes:

```text
Repository maturity:
GREENFIELD | BROWNFIELD

Product workflow:
UI_FIRST | UX_FIRST
```

These dimensions are orthogonal.

Matrix:

```text
                     UI_FIRST                              UX_FIRST

GREENFIELD   UX contract -> shell design -> dev    UX execution -> temp UI
                                                -> collect requirements
                                                -> UI Update Packs

BROWNFIELD   inspect live UX + shell              inspect live UX
             -> keep/upgrade shell                -> temp UI
             -> dev                               -> collect requirements
                                                -> UI Update Packs
```

Changing an already-approved UI_FIRST/UX_FIRST classification is a material planning decision and requires Human approval.

---

# 4. Shared prerequisite: UX execution contract

Before final visual architecture is designed, Planner/Designer must understand what the product actually needs to execute.

The UX execution contract answers:

- what the user does;
- what information appears;
- which content regions exist;
- navigation structure;
- primary/secondary actions;
- state transitions;
- empty/loading/error/success/disabled states;
- content density;
- variable-length cases;
- responsive behavioral needs;
- persistent vs replaceable regions;
- product constraints that affect component/shell boundaries.

This contract is not visual styling.

Core rule:

> UI must be designed from UX execution needs, not from visual guessing.

---

# 5. UI_FIRST target workflow

Use UI_FIRST when the visual shell/system should become an early stable product contract.

Target flow:

```text
objective
-> Planner
-> UX execution contract
-> Designer
-> Design Coverage
-> DESIGN_READY
-> reviewed Designer package
-> UI DESIGN PHASE
-> UI Coder + Penpot
-> Tester shell checkpoint
-> Human design loop
-> UI DESIGN APPROVED
-> source implementation
-> representative real-content pilot
-> practical verification
-> SHELL_READY
-> scale-out / later content
```

The shell should anticipate the real execution needs sufficiently that later content/features can fit without repeated structural redesign.

---

# 6. UX_FIRST target workflow

Use UX_FIRST when product behavior/flow needs to evolve before final visual architecture should be frozen.

Target flow:

```text
objective
-> Planner
-> UX/product-flow execution
-> TEMPORARY_UI proposal
-> Human approval for material interaction choices
-> Coder implements provisional UI
-> observe real UX needs
-> collect DURABLE_UI_REQUIREMENT
-> accumulate across Jobs
-> UI Update Pack trigger
-> Planner forms coherent pack
-> Designer
-> Human review when material
-> UI Coder
-> visual/regression verification
```

Key rule:

> Temporary UI is execution scaffolding, not automatically permanent visual architecture.

Do not redesign UI after every UX change.

Do not allow temporary UI to become permanent accidentally.

Use:

```text
collect
-> group
-> periodic coherent UI Update Pack
```

---

# 7. Design Coverage

The new architecture separates "design artifacts" from "design coverage".

Design Coverage means the known required UI space is sufficiently covered.

Applicable coverage includes:

- required screen types;
- flows;
- shared shell;
- important states;
- responsive variants;
- accessibility-relevant states;
- media-heavy states;
- representative content variation;
- asset slots/requirements.

It does not mean every production asset or every generated content item must already exist.

---

# 8. DESIGN_READY

`DESIGN_READY` means:

- known required surfaces/states are covered;
- visual system is coherent enough to implement;
- responsive intent is sufficiently defined;
- major visual decisions are not left to Coder;
- asset contracts/slots are sufficiently defined.

It does not require every final binary asset.

Stable placeholders/contracts may still be valid.

---

# 9. New UI Design Phase

The major Online UI Coder upgrade inserts a practical design-authoring phase after reviewed Designer output and before source UI implementation.

Conceptual flow:

```text
Designer package
-> Reviewer PASS where required
-> UI DESIGN PHASE
-> UI Coder + Penpot
-> Tester
-> Human
<-> UI Coder / Tester iteration
-> UI DESIGN APPROVED
-> source implementation
```

Penpot expands UI Coder capability.

It does not replace Designer.

It does not turn UI Coder into the owner of product requirements.

---

# 10. Penpot responsibilities

During the UI Design Phase, UI Coder may use Penpot to:

- build application shell;
- create all distinct required screen types;
- build reusable visual components;
- apply/create visual tokens;
- define visual state variants;
- realize responsive compositions;
- place representative content;
- create/export shell-level UI assets;
- integrate approved generated assets;
- prepare prototype/shell for Tester/Human review;
- use design-to-code output where useful later.

The UI Coder capability is broadened, not narrowed.

---

# 11. Full screen coverage + representative content

The UI Design Phase follows:

```text
FULL SCREEN COVERAGE
+
REPRESENTATIVE CONTENT COVERAGE
```

Every distinct required product screen type must be represented.

Repeated/generated content does not need exhaustive Penpot instances.

Representative samples should be sufficient to expose:

- layout;
- hierarchy;
- density;
- state treatment;
- responsive behavior;
- asset placement;
- interaction affordances.

This keeps Penpot from becoming a duplicate content database.

---

# 12. Asset model

The target architecture distinguishes three asset classes.

## 12.1 Penpot-native shell/UI assets

Examples:
- icons;
- simple vectors;
- badges;
- card decorations;
- progress visuals;
- shared shell artwork.

These may be created/exported directly from Penpot and used in source.

## 12.2 Reviewed generated-asset contracts

If Designer already defines stable:

```text
asset ID
filename
prompt
usage
path/slot
ratio/dimensions
```

UI Coder should preserve that contract.

The workflow is:

```text
approved prompt/contract
-> generate asset
-> preserve identity/filename
-> use real asset in design/source
```

Do not casually replace a reviewed background/hero/artwork contract with an unrelated icon/vector.

## 12.3 Content-deep assets

Assets belonging to deep/generated content rather than the shell continue through the content asset-generation pipeline.

The UI design phase should not absorb the entire content-generation architecture.

---

# 13. Tester checkpoint inside UI design

After the Penpot shell becomes testable, Tester enters at a meaningful UI checkpoint.

Tester focuses on observable evidence:

- required screens exist;
- prototype/navigation flow works where supported;
- visual states exist;
- responsive behavior;
- clipping/overflow;
- obvious layout breakage;
- representative flow completion;
- screenshots at relevant viewports;
- concise objective observations.

Tester does not make subjective visual acceptance decisions.

---

# 14. Human design loop

Target loop:

```text
UI Coder
-> Penpot shell
-> Tester
-> screenshots + objective evidence
-> Human
-> feedback
-> UI Coder revises
-> Tester retests
-> Human
-> ...
```

Routine visual refinement does not require Reviewer re-entry every iteration.

Examples:
- spacing;
- sizing;
- density;
- color treatment;
- icon sizing;
- composition;
- artwork placement.

Material upstream changes do require causal replanning/review.

Examples:
- screen set changes;
- navigation topology changes;
- product flow changes;
- state semantics change;
- major responsive behavior changes;
- reviewed asset contract changes.

---

# 15. UI DESIGN APPROVED

After Tester/Human loop:

```text
UI DESIGN APPROVED
```

means the Penpot artifact is the accepted visual implementation reference.

Source implementation may then use:

- approved Penpot design;
- exported UI assets;
- approved generated assets;
- design-to-code output where useful;
- existing Designer package/contracts.

Coder should no longer invent the shell from scratch in source.

---

# 16. SHELL_READY

`SHELL_READY` is later than DESIGN_READY and UI DESIGN APPROVED.

Target concept:

```text
DESIGN_READY
-> shell implementation
-> representative real-content pilot
-> practical verification
-> SHELL_READY
-> scale-out
```

This prevents scaling many screens/features on top of an unproven shell.

Tester performs practical verification only when the required runtime capability is actually observed.

Otherwise the workflow creates a Human Retest/evidence obligation.

---

# 17. Capability readiness model

The target Tester/UI architecture uses a finer readiness progression:

```text
DISCOVERED
-> REACHABLE
-> INVOCABLE
-> FUNCTIONAL
-> GOAL_MET
```

Tool presence is not proof.

Examples:
- Penpot MCP visible = DISCOVERED only;
- browser tool visible = DISCOVERED only;
- bridge reachable = REACHABLE;
- operation callable = INVOCABLE;
- operation works against intended target = FUNCTIONAL;
- entire checkpoint promise succeeds = GOAL_MET.

The existing `SURFACE_PRESENT / OBSERVED` vocabulary may remain temporarily for compatibility.

---

# 18. Tester target architecture

The planned Online Tester becomes a checkpoint-level evidence agent, not a thin browser verifier.

Target lifecycle:

```text
GROUND
-> PLAN
-> CAPABILITY
-> AUTHOR
-> EXECUTE
-> ANALYZE
-> ADAPT
-> REPORT
-> RETEST
```

Modes:

```text
VERIFY
MEASURE
PROBE
```

Tester outputs can be dependencies for later Planner/Coder work.

Unknown runtime facts must survive planning as first-class EvidenceNeed records.

Tester-discovered PLAN_GAP/SPEC_GAP routes back to Planner.

Tester never rewrites topology itself.

---

# 19. UI Tester and capability probing

Before Penpot/browser evidence is required by workflow, capability itself may need a PROBE checkpoint.

Examples:

```text
Can Penpot be reached?
Can UI Coder create/edit/read back a real design?
Can assets be exported?
Can browser runtime open the real target?
Can screenshots be captured?
Can viewport switching work?
Can resulting evidence be persisted?
```

This is especially important for the first implementation.

Do not make a future phase depend on an unproven tool surface.

---

# 20. Browser QA target

Online target browser QA is an internal GSA capability.

Intended uses:

## Designer

`browser_ui_inspect`

Returns:
- compact accessibility snapshot;
- screenshots for selected viewports;
- console output;
- page errors.

## Tester

`browser_test_flow`

Runs bounded non-destructive UI flows:
- click;
- fill;
- check;
- hover;
- select;
- press;
- wait;
- assert visible text.

Evidence rule:

```text
tool exists
!=
capability proven
```

Only a successful call against the exact target/state is observed evidence.

---

# 21. Planner changes required by the target UI model

Before persisting a UI-bearing Job, Planner should be able to answer:

1. Is the Project UI_FIRST or UX_FIRST?
2. What is the UX execution contract?
3. What UI requirements follow from it?
4. What existing shell/design system is authoritative?
5. Is DESIGN_READY already satisfied?
6. Does this work require Designer?
7. Does this work require Penpot UI Design Phase?
8. What Tester checkpoint proves shell/prototype readiness?
9. What requires Human subjective acceptance?
10. When is UI DESIGN APPROVED?
11. When is SHELL_READY?
12. What asset contracts already exist?
13. Which assets may be Penpot-native?
14. Which asset prompts/IDs must be preserved?
15. What capability must be PROBED before later phase dependencies are allowed?

For UX_FIRST additionally:

16. What TEMPORARY_UI is required now?
17. Which parts are explicitly provisional?
18. What requires Human approval now?
19. Where are durable UI requirements recorded?
20. What triggers a UI Update Pack?

---

# 22. Topology implications

The target architecture does not require one rigid topology for every UI Job.

Possible UI_FIRST topology:

```text
Planner
-> UX execution contract
-> Designer
-> Design Review / Human Gate where needed
-> UI Design Phase
-> UI Coder + Penpot
-> Tester shell checkpoint
-> Human design acceptance
-> UI source implementation
-> representative pilot
-> Tester/runtime verification
-> SHELL_READY
-> scale-out
```

Possible UX_FIRST topology:

```text
Planner
-> UX work
-> TEMPORARY_UI proposal
-> Human approval where material
-> Coder
-> observed UX evidence
-> durable UI requirements
-> later UI Update Pack
-> Designer
-> UI Coder + Penpot
-> Tester
-> Human
-> source refinement
```

Detailed Execution Plan chooses causal order.

There is no universal fixed UX-Coder-then-UI-Coder rule.

---

# 23. Relationship to GSA Local resume/recovery lessons

The later GSA Local total replan must reconcile the UI target architecture with the already-reviewed workflow lessons.

Important combinations:

## 23.1 Human design feedback can become Human Rebaseline

Routine visual feedback stays inside the UI Design loop.

Material changes to:
- product flow;
- screen topology;
- state semantics;
- architecture;
- ownership;
- major responsive behavior;

must trigger the existing Human Rebaseline + Salvage contract.

Do not silently keep implementing the old architecture.

## 23.2 UI Design approval creates a useful integration baseline

`UI DESIGN APPROVED` is a natural durable design baseline.

`SHELL_READY` is a later runtime/product baseline.

These can align with bounded integration checkpoints.

## 23.3 Tester UI evidence follows evidence applicability

Screenshots and browser evidence must bind to:
- exact target/state;
- viewport/device dimensions;
- relevant configuration;
- design/prototype/source revision;
- capability level.

Do not reuse stale screenshot evidence after material design/source changes.

## 23.4 UI work must remain resumable

A WorkCursor for UI work may need stages such as:

```text
DESIGNER
DESIGN_REVIEW
UI_DESIGN
UI_TEST_CHECKPOINT
WAITING_HUMAN_DESIGN
UI_DESIGN_APPROVED
UI_IMPLEMENTATION
SHELL_PILOT
SHELL_VERIFICATION
SHELL_READY
```

Resume must return to the exact stage rather than restarting design or source implementation.

## 23.5 Penpot/external browser operations need durable ExternalOperation identity

Long-lived external design/testing operations should follow the workflow lessons:
- provider operation identity;
- target binding;
- replay policy;
- current status;
- result/evidence refs.

---

# 24. Important difference between target architecture and current Local plan

Current GSA Local plan does not yet contain first-class concepts for:

```text
Designer
UX Coder
UI Coder
UI_FIRST / UX_FIRST
DESIGN_READY
UI DESIGN PHASE
Penpot
UI DESIGN APPROVED
SHELL_READY
TEMPORARY_UI
UI Update Pack
browser UI inspection
Human visual acceptance loop
```

Therefore later migration should not be a small patch to one Phase.

It requires a deliberate rebaseline of:
- role model;
- Planner contract;
- Job Builder topology;
- checkpoint model;
- Tester evidence;
- Human Gates;
- workflow resume states;
- integration baselines;
- capability probing.

---

# 25. What should remain unchanged unless later review says otherwise

The Online target architecture explicitly preserves several existing principles:

- Designer remains distinct from UI Coder.
- Tester does not replace Human subjective design approval.
- UI Coder remains a Coder specialization rather than a mandatory top-level Job type.
- Tester is not inserted after every Coder task.
- Coder lightweight self-check remains.
- Reviewer remains context-isolated where required.
- CR remains Human-invoked only.
- unavailable UI/browser capability remains UNVERIFIED/BLOCKED rather than fabricated PASS.
- existing reviewed asset contracts remain authoritative.
- detailed methodology belongs to specialist harness/skills, not universal core prompts.

These should be treated as preservation constraints during the later Local replan.

---

# 26. Questions the later Local total replan must resolve

The later full plan rewrite should explicitly decide:

1. Should Local adopt Designer/UX Coder/UI Coder now or stage them later?
2. Is Penpot available to Local runtime directly, indirectly, or only to Online GSA?
3. What capability PROBE is required before Penpot becomes a dependency?
4. Where does Design Coverage live in Local Registry/plan artifacts?
5. What exact fields represent UI_FIRST/UX_FIRST?
6. What is the Local equivalent of UI DESIGN APPROVED?
7. What is the Local equivalent of SHELL_READY?
8. How are Human visual gates represented/resumed?
9. How are Penpot artifacts identified and version-bound?
10. How are screenshots/evidence stored locally?
11. How does Tester switch viewports and persist evidence?
12. How are generated asset contracts represented?
13. Which shell assets may be produced from Penpot?
14. How does UI Update Pack interact with Milestone/Job Pack topology?
15. How does Human Rebaseline affect approved UI design and source work?
16. Which UI checkpoints should become AcceptedIntegrationBaselines?
17. How does Local avoid duplicating Designer behavior inside UI Coder?
18. Which Online contracts are target architecture only and need Local empirical validation before adoption?

---

# 27. Recommended use of this file

When the Local total replan begins:

```text
WORKFLOW_LESSONS_LEARNED.md
+
this file
+
current GSA Local live code/plan
+
current GSA Online target architecture
+
observed Local/Online capabilities
-> new architecture rebaseline
-> revised total implementation plan
```

This file should remain a reference note.

Do not treat it as the implementation plan itself.

---

# Core conclusion

The newer GSA Online architecture changes the UI model from:

```text
Planner
-> Coder implements UI
-> Tester verifies later
```

toward:

```text
Planner defines UX/product contract
-> Designer defines visual/design coverage
-> UI Coder realizes a real UI shell in Penpot
-> Tester objectively verifies the shell
-> Human accepts subjective UI
-> source implementation follows the approved design
-> representative pilot
-> practical verification
-> SHELL_READY
```

while UX_FIRST Projects may deliberately delay final visual design and accumulate UI requirements into coherent update packs.

This architecture should be considered before any broad rewrite of the GSA Local plan.
