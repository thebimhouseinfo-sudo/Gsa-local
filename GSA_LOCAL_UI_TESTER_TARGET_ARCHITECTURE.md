# GSA Local — UI Coder + Penpot + Tester agent-browser Target Architecture

## Status

PURPOSE: target architecture input for the future GSA Local total replan  
AUTHORITY: NON-CANONICAL / DESIGN TARGET  
IMPLEMENTATION: NOT STARTED  
CODE CHANGES: NONE FROM THIS DOCUMENT  
PLAN MIGRATION: NOT YET PERFORMED

This file records the intended GSA Local UI/UX + Tester direction after comparing the current Local architecture with the newer GSA Online target architecture.

It is not an implementation plan.

---

# 1. Target decision

GSA Local should adopt the newer UI workflow direction.

Target role model:

~~~text
Planner
Designer
Coder
  ├─ UX Coder
  └─ UI Coder
Reviewer
Tester
Orchestrator
~~~

Local-specific execution choices:

~~~text
UI Coder visual workspace = Penpot
Tester browser executor    = Vercel agent-browser MCP
~~~

Penpot does not replace Designer.

agent-browser does not replace Tester.

---

# 2. Core UI-FIRST workflow

Target Local UI-FIRST flow:

~~~text
Objective
-> Planner
-> UX execution contract
-> Designer
-> Design Coverage
-> DESIGN_READY
-> required Review/Human Gate
-> UI DESIGN PHASE
-> UI Coder + Penpot
-> Tester checkpoint
-> screenshots + observed evidence
-> Human design review
<-> UI Coder / Tester refinement loop
-> UI DESIGN APPROVED
-> source implementation
-> representative real-content pilot
-> Tester practical verification
-> SHELL_READY
-> scale-out
~~~

Source UI should be implemented from an approved design reference rather than invented inside source code.

---

# 3. UI-FIRST and UX-FIRST

Every UI-bearing Project should be classified independently on two axes:

~~~text
Repository maturity:
GREENFIELD | BROWNFIELD

Product workflow:
UI_FIRST | UX_FIRST
~~~

## UI-FIRST

Use when the visual shell/system should become an early stable product contract.

~~~text
UX execution contract
-> Designer
-> Penpot UI Design
-> Human approval
-> implementation
-> shell verification
-> scale-out
~~~

## UX-FIRST

Use when interaction/product behavior must evolve before final visual architecture should be frozen.

~~~text
UX work
-> approved TEMPORARY_UI
-> Coder
-> observe actual needs
-> DURABLE_UI_REQUIREMENT backlog
-> UI Update Pack
-> Designer
-> UI Coder + Penpot
-> Tester
-> Human
-> coherent visual implementation
~~~

Temporary UI must not silently become permanent visual architecture.

---

# 4. Planner contract for UI-bearing work

Before a UI-bearing Job/JobPack becomes executable, Planner should resolve:

1. GREENFIELD or BROWNFIELD?
2. UI_FIRST or UX_FIRST?
3. What is the UX execution contract?
4. What shell/design system already exists?
5. What is canonical vs legacy/accidental?
6. What screen/state coverage is required?
7. Is Designer required?
8. What constitutes DESIGN_READY?
9. Is a Penpot UI Design Phase required?
10. Which UI choices require Human approval?
11. What Test Checkpoint verifies the shell/prototype?
12. What exact evidence must Tester return?
13. What defines UI DESIGN APPROVED?
14. What representative pilot proves the shell?
15. What defines SHELL_READY?
16. What asset contracts already exist?
17. Which assets may be Penpot-native?
18. Which assets remain deep-content assets?
19. What Penpot capability must be PROBED first?
20. What agent-browser capability must be PROBED first?

For UX_FIRST additionally:

21. What TEMPORARY_UI is required now?
22. Which parts are provisional?
23. Which visible interaction choices need Human approval?
24. Where are DURABLE_UI_REQUIREMENT records stored?
25. What threshold/milestone forms a UI Update Pack?

---

# 5. Designer boundary

Designer remains a distinct source-read-only role.

Designer owns:
- visual interpretation;
- Design Coverage;
- visual language/system;
- responsive intent;
- artwork/icon language;
- asset requirements;
- visual acceptance references.

Designer does not:
- edit product source;
- own product behavior;
- silently change Planner-owned UX requirements;
- become the Penpot implementation Coder by default.

Designer output becomes an implementation constraint for UI Coder.

---

# 6. UI Coder boundary

UI Coder remains a Coder specialization.

UI Coder owns:
- Penpot realization of reviewed design;
- visual composition;
- responsive visual composition;
- reusable visual components;
- visual tokens;
- asset placement;
- exported shell/UI assets;
- source implementation after design approval.

UI Coder does not own unresolved product requirements.

Material design conflict routes back to Planner/Designer/Human.

---

# 7. Penpot UI Design Phase

Penpot becomes the primary visual authoring workspace for GSA Local UI Coder.

Target operations include:
- create application shell;
- create distinct required screen types;
- create reusable components;
- apply/create design tokens;
- define visual state variants;
- realize responsive layouts;
- place representative content;
- integrate shell-level assets;
- export production-usable assets where appropriate;
- prepare prototype/reference for Tester and Human;
- use design-to-code output where useful.

Penpot capability must first be PROBED.

Readiness progression:

~~~text
DISCOVERED
-> REACHABLE
-> INVOCABLE
-> FUNCTIONAL
-> GOAL_MET
~~~

Before Penpot becomes a mandatory dependency, Local must empirically prove at least:
- create/open design;
- edit design;
- read back design state;
- preserve project/file identity;
- export required assets;
- recover/resume after a new Local session/process;
- expose an inspectable prototype/reference to Tester/Human.

---

# 8. Full screen coverage + representative content

Penpot phase follows:

~~~text
FULL SCREEN COVERAGE
+
REPRESENTATIVE CONTENT COVERAGE
~~~

All materially distinct required screen types should be represented.

Repeated/generated content does not need exhaustive design instances.

Representative data should expose:
- content density;
- short/long content;
- key states;
- responsive behavior;
- artwork placement;
- interaction affordances.

Penpot must not become a duplicate content database.

---

# 9. Asset contract

GSA Local should distinguish three asset classes.

## 9.1 Penpot-native shell assets

Examples:
- icons;
- vectors;
- badges;
- UI decoration;
- progress visuals;
- shell artwork.

These may be created/exported from Penpot.

## 9.2 Reviewed generated-asset contracts

If Designer already defines:
- asset_id;
- filename;
- prompt;
- usage;
- path/slot;
- ratio/dimensions;

UI Coder preserves that contract.

Target flow:

~~~text
reviewed prompt/identity
-> generate real asset
-> preserve filename/ID
-> place into Penpot/source
~~~

Do not replace a specified background/hero/card artwork with a generic vector merely because Penpot can create one.

## 9.3 Deep content assets

Lesson/content illustrations, media sets and similar deep assets remain in the content-generation pipeline.

UI Coder should not absorb unrelated content generation.

---

# 10. Tester remains an independent agent

Tester is not merely a browser script runner.

Tester owns:

~~~text
GROUND
-> PLAN
-> CAPABILITY
-> AUTHOR
-> EXECUTE
-> ANALYZE
-> ADAPT
-> REPORT
-> RETEST
~~~

Modes remain:

~~~text
VERIFY
MEASURE
PROBE
~~~

Tester:
- independently derives test plans;
- writes test-only artifacts in Tester workspace;
- chooses agent-browser flows;
- evaluates observations against goal/spec;
- classifies failures;
- persists evidence;
- produces next-phase inputs.

Vercel agent-browser MCP is the execution capability, not the decision-maker.

---

# 11. Primary browser executor: Vercel agent-browser MCP

GSA Local Tester should use Vercel agent-browser MCP as the primary browser automation layer.

Expected interaction pattern:

~~~text
open target
-> wait for stable load
-> snapshot interactive state
-> interact
-> re-snapshot after navigation/DOM changes
-> assert observed state
-> screenshot
-> inspect console/page evidence
-> report
~~~

Useful capability classes include:
- navigation;
- interactive snapshots;
- click/fill/select/check/press;
- waiting on load/url/text;
- text/url/title inspection;
- screenshots;
- annotated screenshots;
- screenshot diff;
- browser session/state persistence;
- bounded JavaScript evaluation when justified.

Element references are transient.

After navigation or material DOM changes:

~~~text
old refs invalid
-> new snapshot
-> use new refs
~~~

Tester harness must enforce this behavior.

---

# 12. agent-browser capability must be proven

Before Planner makes agent-browser a required checkpoint dependency, Tester should run a standalone PROBE.

Minimum probe:

~~~text
Can agent-browser start?
Can it open the target?
Can it wait for stable load?
Can it create interactive snapshots?
Can it click/fill?
Can it re-snapshot?
Can it exercise required viewport/device conditions?
Can it save screenshots?
Can it return the saved artifact path?
Can it inspect console/page errors?
Can state/session be reused when required?
Can it close/cleanup deterministically?
~~~

Tool presence is not FUNCTIONAL evidence.

If a required operation is unavailable:

~~~text
classification = INTEGRATION_NOT_READY
~~~

not PASS.

---

# 13. Tester UI evidence workspace

UI/browser evidence should live inside Tester-owned workspace.

Suggested structure:

~~~text
tester-workspace/
  <job-or-jobpack>/
    <checkpoint>/
      plan/
      tests/
      fixtures/
      browser/
        sessions/
        snapshots/
        screenshots/
        diffs/
        console/
      report/
~~~

Product source remains read-only.

Browser evidence must not be scattered into arbitrary project directories.

---

# 14. Screenshot paths are user-facing workflow outputs

After UI design checkpoints, Human must be able to inspect actual output directly.

Tester report should include explicit evidence paths such as:

~~~text
desktop_screenshot_path
tablet_landscape_screenshot_path
tablet_portrait_screenshot_path
mobile_screenshot_path
annotated_screenshot_path?
diff_path?
snapshot_path?
~~~

Human-facing response must surface the relevant path directly.

The Human should not need to search the Tester workspace manually.

Each screenshot should bind to:
- checkpoint;
- exact SourceTargetSet/design target;
- viewport;
- browser operation/session;
- capability state;
- timestamp.

---

# 15. Viewport and responsive verification

Tester derives viewport classes from the approved responsive contract.

Possible classes:
- mobile portrait;
- tablet portrait;
- tablet landscape;
- laptop/desktop;
- large display.

Do not hard-code one universal viewport matrix for every product.

Planner/Designer specify semantic responsive obligations.

Tester selects sufficient concrete viewport sizes.

For each tested viewport collect as applicable:
- screenshot;
- overflow/clipping observations;
- visible/hidden behavior;
- hierarchy;
- primary action accessibility;
- console/page errors.

Viewport switching itself must be capability-probed before workflow depends on it.

---

# 16. UI Design Tester checkpoint

After Penpot shell/prototype becomes inspectable, Tester receives a meaningful UI checkpoint.

Tester verifies objective properties:
- required screen types represented;
- required prototype/navigation paths usable where supported;
- visual states exist;
- responsive variants exist;
- obvious clipping/overflow absent;
- interaction targets reachable;
- representative content fits regions;
- required shell assets present;
- known runtime/console issues recorded;
- screenshots captured.

Tester does not decide subjective visual preference.

Those decisions route to Human.

---

# 17. Human design loop

Target Local loop:

~~~text
UI Coder + Penpot
-> Tester agent-browser evidence
-> screenshot paths surfaced to Human
-> Human reviews
-> feedback
-> UI Coder revises Penpot
-> Tester retests
-> Human reviews
-> ...
~~~

Routine visual changes stay inside this loop.

Examples:
- spacing;
- sizing;
- density;
- icon scale;
- decorative placement;
- color tuning;
- composition refinements.

Material feedback routes to Planner/Rebaseline when it changes:
- screen topology;
- product flow;
- state semantics;
- component ownership;
- major responsive rules;
- asset contracts;
- architecture.

---

# 18. UI DESIGN APPROVED

After required Tester evidence and Human acceptance:

~~~text
UI DESIGN APPROVED
~~~

This becomes a durable design baseline.

Suggested binding:
- ui_design_baseline_id;
- Penpot project/file identity;
- Penpot revision/version identity if available;
- design coverage reference;
- Tester verification refs;
- Human approval ref;
- asset contract refs;
- responsive contract;
- approved_at.

Source implementation binds to this baseline.

A material later design change explicitly invalidates/supersedes it.

---

# 19. Source implementation after UI approval

After UI DESIGN APPROVED:

~~~text
UI Coder
-> read approved Penpot baseline
-> export/use approved assets
-> implement source
-> lightweight Coder self-check
-> Reviewer
-> planned checkpoints
~~~

Penpot design-to-code output may accelerate implementation.

It is not automatically canonical source.

Coder still owns:
- code quality;
- framework conventions;
- accessibility implementation;
- source responsive behavior;
- regression safety.

---

# 20. Representative pilot before SHELL_READY

Do not immediately scale every screen/content item after shell implementation.

Use:

~~~text
implemented shell
-> representative real-content pilot
-> Tester using agent-browser
-> Human review where needed
-> SHELL_READY
-> scale-out
~~~

Pilot content should expose:
- long text;
- short text;
- media;
- empty/error/loading states;
- representative navigation;
- responsive stress.

SHELL_READY is stronger than UI DESIGN APPROVED because it has runtime evidence.

---

# 21. Tester after source implementation

Tester uses agent-browser against the runnable application to verify:
- navigation;
- click/fill/keyboard flows;
- visible/hidden states;
- responsive behavior;
- clipping/overflow;
- focus/interaction where observable;
- required assets;
- console/page errors;
- representative user-flow completion;
- screenshots across required viewports.

Where useful:
- snapshot diff;
- screenshot diff;
- session-state reuse;
- repeated-run measurement.

Tester remains checkpoint-driven, not inserted after every small Coder change.

---

# 22. UI evidence applicability

Browser/UI evidence must be dimension-aware.

Suggested dimensions:

~~~text
source_target_set
ui_design_baseline_id
penpot target/version
application URL/build identity
viewport width/height
device class
browser/runtime version where material
theme/mode
locale where material
auth/session state
feature flags/config
asset version
checkpoint
~~~

Changing a relevant dimension may invalidate old screenshots/tests.

Do not reuse evidence only because an old artifact path still exists.

---

# 23. Tester result classification for UI

Use:

~~~text
PRODUCT_FAILURE
TEST_FAILURE
ENVIRONMENT_FAILURE
NOT_READY
INTEGRATION_NOT_READY
SPEC_GAP
PLAN_GAP
NEEDS_HUMAN
~~~

Examples:
- wrong behavior against approved flow -> PRODUCT_FAILURE;
- stale element ref after navigation -> TEST_FAILURE;
- agent-browser cannot launch -> ENVIRONMENT_FAILURE;
- Penpot prototype not inspectable yet -> INTEGRATION_NOT_READY;
- no approved mobile behavior -> SPEC_GAP;
- required UI checkpoint absent from plan -> PLAN_GAP;
- subjective composition choice -> NEEDS_HUMAN.

---

# 24. agent-browser test authoring

Tester should create its own browser test plan and reusable test definitions where useful.

Plan may include:
- preconditions;
- target URL;
- session requirements;
- viewport set;
- navigation path;
- expected visible states;
- interaction steps;
- snapshot points;
- screenshot points;
- failure evidence;
- cleanup;
- replay safety.

Product Coder does not edit Tester-owned test definitions merely to make the checkpoint pass.

If a test should become product-owned permanent regression coverage, Tester reports that requirement and Coder implements it in product tests.

---

# 25. Session/state handling

agent-browser supports persistent browser sessions/state where the runtime exposes those operations.

Use only when the test contract requires it.

Examples:
- authenticated flow;
- multi-page state;
- same-session measurements;
- recovery flow.

Persist enough identity to diagnose/resume:
- browser_operation_id;
- agent_browser_session_id;
- state_file_path if applicable;
- target URL;
- checkpoint;
- replay policy;
- last stable snapshot.

Do not assume browser daemon/session survives process restart.

If continuity matters, PROBE it and record evidence.

---

# 26. ExternalOperation relationship

Penpot and agent-browser operations should follow the Local recovery lessons.

Suggested conceptual record:

~~~text
ExternalOperation
  provider = PENPOT | AGENT_BROWSER
  operation_id
  provider_session_or_object_id
  target
  requested_by_run
  status
  replay_policy
  artifact_refs
  last_observed_at
~~~

Resume should inspect existing operation identity before recreating it.

---

# 27. Resume-aware UI state machine

Local WorkCursor should eventually support UI stages such as:

~~~text
UX_CONTRACT
DESIGNER
DESIGN_REVIEW
DESIGN_READY
UI_DESIGN
UI_TEST_CHECKPOINT
WAITING_HUMAN_DESIGN
UI_DESIGN_REVISION
UI_DESIGN_APPROVED
UI_IMPLEMENTATION
SOURCE_REVIEW
SHELL_PILOT
SHELL_TEST
WAITING_HUMAN_SHELL
SHELL_READY
UI_UPDATE_PACK
~~~

A new session resumes the exact stage.

It must not:
- restart Penpot design from zero;
- skip pending Human review;
- blindly repeat already-valid Tester evidence;
- jump from DESIGN_READY directly to source when UI Design Phase is required;
- reactivate an old design after Human Rebaseline.

---

# 28. Human Rebaseline interaction

Routine UI refinement stays inside the design loop.

Material Human feedback becomes:

~~~text
HUMAN_REBASELINE_REQUIRED
~~~

Then:

~~~text
freeze affected UI/source work
-> preserve current Penpot/source artifacts
-> classify salvageability
-> impact analysis
-> Planner revised contract
-> required review
-> new graph/design epoch
-> explicitly adopt reusable design/source/evidence
-> continue
~~~

Do not delete all prior design/code automatically.

Do not preserve obsolete design authority automatically.

---

# 29. Bounded integration checkpoints for UI work

Natural accepted UI baselines include:

~~~text
DESIGN_READY
UI DESIGN APPROVED
implemented shell + Reviewer PASS
SHELL_READY
coherent UI Update Pack completion
~~~

Not all are literal Git merges.

They are stable accepted integration/promotion boundaries.

Avoid one giant work lineage with hundreds of mixed:
- design iterations;
- asset changes;
- source changes;
- browser fixes;
- responsive fixes.

Prefer bounded reviewed slices.

---

# 30. UI Update Pack

For UX_FIRST projects, durable UI requirements accumulate until a coherent update boundary.

Suggested concepts:

~~~text
UIRequirement
  requirement_id
  source_job/task
  affected_surface
  reason
  provisional_ui_ref?
  evidence_refs[]
  priority
  status

UIUpdatePack
  pack_id
  requirement_ids[]
  affected_shell/surfaces[]
  scope
  non_goals
  design_gate
  implementation_gate
  verification_gate
~~~

Planner owns grouping.

Designer/UI Coder should receive a coherent pack rather than unrelated styling requests.

---

# 31. Standalone capability probes before full adoption

Before this target architecture becomes mandatory, run standalone capability validation.

## Penpot probe

Prove:
- project/file create/open;
- edit;
- readback;
- component/state creation;
- responsive frame handling;
- asset export;
- stable object/file identity;
- artifact retrieval after new Local session;
- inspectable prototype/reference.

## agent-browser probe

Prove:
- MCP/tool availability;
- browser launch;
- open target;
- network-idle wait;
- snapshot;
- click/fill;
- re-snapshot;
- required viewport switching;
- screenshot;
- annotated screenshot where useful;
- screenshot path recovery;
- console/page errors;
- state save/load where required;
- deterministic cleanup;
- bounded timeout/error behavior.

Do not design later phases around unsupported operations.

---

# 32. Tester should test UI Coder capability itself

For initial rollout, Tester independently verifies UI Coder + Penpot capability before Planner assumes it.

Target standalone pilot:

~~~text
Planner:
  declares which UI Coder capabilities must be known

Tester:
  -> creates capability test plan
  -> defines tasks UI Coder must perform
  -> defines required returned artifacts/evidence

UI Coder:
  -> performs Penpot tasks

Tester:
  -> independently inspects outputs
  -> uses agent-browser/screenshot capability where applicable
  -> measures what is actually usable
  -> returns capability matrix

Planner:
  -> uses observed capability matrix for later topology
~~~

This tests both:
- Tester independence;
- UI Coder/Penpot real capability.

It should be standalone before deeply wiring Planner orchestration.

---

# 33. Human evidence delivery

For UI checkpoints, Tester handoff should be Human-friendly.

Suggested output:

~~~text
Checkpoint: UI-SHELL-CP1
Target: <exact design/source target>

Desktop:
  screenshot: <path>

Tablet landscape:
  screenshot: <path>

Tablet portrait:
  screenshot: <path>

Mobile:
  screenshot: <path>

Observed:
  - ...
  - ...

Blocked/unverified:
  - ...

Human action:
  Review screenshots and approve or request visual changes.
~~~

The system must surface file paths directly.

The Human should not search internal Tester directories manually.

---

# 34. Preservation constraints

Keep:
- Coder lightweight self-check;
- Reviewer source/contract review;
- Tester checkpoint-level independence;
- Tester source read-only;
- Tester-owned workspace;
- CR Human-only;
- exact target binding;
- EvidenceNeed/UnknownRuntimeFact;
- meaningful sparse checkpoints;
- Rebaseline/Salvage;
- bounded integration;
- resume/recovery contracts;
- Human merge/release authority;
- UNVERIFIED != PASS.

---

# 35. What the future total replan must migrate

The later GSA Local architecture replan should integrate this target into:
- role model;
- Planner contracts;
- PlanArtifact;
- Job Builder;
- ExecutionGraph;
- TestCheckpointSpec;
- WorkCursor;
- ResumeDescriptor;
- evidence applicability;
- ExternalOperation;
- Human Gates;
- design baselines;
- integration baselines;
- task topology;
- capability probing;
- harness/skill layout.

This should be a deliberate architecture rebaseline, not an isolated Penpot patch.

---

# 36. Likely causal implementation sequence for the later plan

This is not yet the execution plan.

Likely sequence:

~~~text
A. Rebaseline Local architecture contracts
B. Add role/workflow model for Designer + UX/UI Coder
C. Add UI_FIRST/UX_FIRST planning contract
D. Add Design Coverage/readiness records
E. Integrate Penpot capability into UI Coder
F. Run standalone UI Coder/Penpot capability PROBE
G. Integrate Vercel agent-browser MCP into Tester executor
H. Run standalone Tester/agent-browser capability PROBE
I. Add UI Design TestCheckpoint + Human design loop
J. Add UI DESIGN APPROVED baseline
K. Bind source implementation to approved design
L. Add representative shell pilot
M. Add agent-browser source UI verification
N. Add SHELL_READY
O. Add UX_FIRST UI Requirement / Update Pack workflow
P. Integrate resume/rebaseline/integration-baseline behavior
Q. Run real-project pilot
~~~

Important:

Do not deeply wire Planner to Penpot/agent-browser before the standalone capability probes prove what actually works.

---

# Core invariant

The intended Local UI workflow is:

~~~text
Planner defines what the product must do
-> Designer defines what the visual system should mean
-> UI Coder realizes the design in Penpot
-> Tester independently verifies observable behavior using Vercel agent-browser MCP
-> Human decides subjective visual acceptance
-> UI Coder implements approved design in source
-> Tester verifies the runnable product
-> SHELL_READY becomes a measured durable baseline
~~~

Penpot is the UI Coder design workspace.

Vercel agent-browser MCP is the Tester browser execution layer.

Neither tool replaces the agent role that reasons about the work.
