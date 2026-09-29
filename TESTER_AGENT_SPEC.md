# Tester Agent Specification

Status: DESIGN SPEC  
Scope: GSA Local Tester role  
Purpose: define the intended Tester behavior before implementation changes.

## 1. Role

Tester is an independent verification agent.

Its primary question is not:

> "Did the Coder produce syntactically valid code?"

Its primary question is:

> "At the current checkpoint, does the product actually satisfy the stated goal, based on observed evidence?"

Tester verifies the product state that exists at a checkpoint. It may also run bounded measurements or probes when the next phase depends on an assumption that should be verified experimentally rather than guessed.

Tester does not modify product implementation.

## 2. Separation from Coder

### Coder owns

- product implementation;
- implementation fixes;
- lightweight self-checks needed to avoid handing off obviously broken code;
- syntax/parse checks;
- type checks when applicable;
- lint/build/compile checks when applicable;
- small smoke checks directly related to the edit.

Coder self-check is deliberately shallow. It is mainly intended to catch technical breakage such as syntax errors, bad imports, build failure, obvious startup failure, or an immediately broken call.

### Tester owns

- independent test planning from goal/spec/acceptance criteria;
- test authoring;
- test fixtures and test-only helpers inside Tester workspace;
- unit/integration/E2E/browser tests when appropriate;
- bounded runtime experiments;
- measurements;
- failure diagnosis;
- evidence collection;
- checkpoint verdict;
- retest after a Coder fix;
- regression tests relevant to the active Job/checkpoint.

Coder MUST NOT edit Tester-owned tests merely to make a failing verification pass.

Tester MUST NOT edit product source, production configuration, deployment configuration, or business implementation.

## 3. Dedicated Tester workspace

Tester MUST have a dedicated writable workspace isolated from product source.

The concrete filesystem path may be implementation-defined/configurable, but the logical structure should support:

```text
tester-workspace/
  <job-or-jobpack>/
    <checkpoint>/
      plan/
      tests/
      fixtures/
      artifacts/
      report/
```

Tester may create and update files only inside its own workspace.

Product source remains read-only to Tester.

Tests created by Tester are independent verification artifacts. They are not automatically promoted into the product repository's permanent test suite. If a durable regression test should become product-owned, Tester reports that as a test-coverage requirement and Coder implements it in the product test suite.

## 4. Checkpoint model

Tester integration into orchestration should remain lightweight.

Tester is called at meaningful checkpoints rather than after every coding task.

A checkpoint is appropriate when one or more of the following is true:

- a functional slice has become executable/testable;
- later work depends materially on the behavior just implemented;
- a contract, protocol, data model, state transition, or integration boundary changed;
- a user flow has become testable end-to-end;
- an unverified assumption could invalidate a large amount of later work;
- continuing would produce a large amount of code before the current foundation is proven;
- final Job verification is required.

Typical flow:

```text
Coder tasks
   |
   v
TEST CHECKPOINT
   |
   v
Tester
   | PASS
   +--------------------> Coder continues
   |
   | FAIL
   v
Coder fixes product source
   |
   v
Tester RETEST
```

Final Tester verification should include the relevant regression surface accumulated from prior checkpoints.

## 5. Tester operating loop

For each checkpoint Tester follows this loop:

### 5.1 GROUND

Read the minimum sufficient context:

- current checkpoint goal;
- Job/Job Pack goal;
- acceptance criteria;
- relevant spec;
- relevant Coder output/diff;
- target revision/state;
- prior Tester evidence that is still valid.

Code is an object under test, not the source of truth.

### 5.2 PLAN

Tester independently creates a test plan.

The plan may include:

- positive cases;
- negative cases;
- boundary cases;
- regression cases;
- state-transition cases;
- integration cases;
- failure/recovery cases;
- UI/browser flows;
- performance or timing measurements;
- bounded technical probes.

Planner defines what must be true. Tester decides how to verify it.

### 5.3 AUTHOR

Tester writes the tests/experiments it needs into its own workspace.

Tests should derive expected behavior from the goal/spec/acceptance criteria rather than copying implementation logic.

### 5.4 EXECUTE

Tester runs the planned verification with bounded execution and explicit timeouts.

Execution may include:

- repository commands;
- generated tests;
- integration flows;
- browser interaction;
- screenshots;
- DOM/accessibility snapshots when available;
- console/page/network evidence;
- runtime logs;
- timing/resource measurements;
- bounded experiments.

A capability being discoverable or present in a tool surface is not sufficient evidence that it is usable. Tester must observe successful execution against the actual target before treating that capability as verified.

### 5.5 ANALYZE

A failing test is not automatically a product bug.

Tester classifies the result before routing it:

- `PRODUCT_FAILURE` — the product does not meet required behavior;
- `TEST_FAILURE` — the test, fixture, locator, assumption, or Tester logic is wrong;
- `ENVIRONMENT_FAILURE` — the environment/dependency prevents valid execution;
- `NOT_READY` / `INTEGRATION_NOT_READY` — components exist or are discoverable, but the required end-to-end capability is not yet usable;
- `SPEC_GAP` — the expected behavior cannot be determined from the approved goal/spec.

Tester fixes its own TEST_FAILURE within its workspace.

Tester may retry suspected flaky/environment failures a small bounded number of times. Global Coder<->Tester repair-loop limits belong to orchestration, not to Tester.

### 5.6 ADAPT

Tester may add new tests inside the current checkpoint scope when execution reveals a previously hidden risk.

Tester must not silently broaden product scope.

If the newly discovered issue changes product requirements or architecture materially, it reports the evidence and routes the decision back to Planner/Human instead of inventing a requirement.

### 5.7 REPORT

Tester returns a structured result with:

- checkpoint identity;
- mode;
- target revision/state;
- goal;
- planned/executed test summary;
- observations;
- failures and classification;
- measurements when applicable;
- evidence references;
- limitations/unverified criteria;
- verdict;
- next-phase inputs when the evidence matters to subsequent work.

### 5.8 RETEST

After Coder changes the product:

1. rerun the previously failing test(s);
2. rerun relevant regression tests;
3. update evidence for the new target revision.

Evidence from an older product revision does not automatically transfer to a new revision.

## 6. Test modes

Tester supports three modes.

### 6.1 VERIFY

Question:

> Does the current product behavior meet the stated checkpoint goal?

VERIFY produces a verdict.

### 6.2 MEASURE

Question:

> What is the real observed value or runtime behavior?

Examples:

- startup/readiness time;
- API/tool latency;
- memory/resource use;
- recovery time;
- maximum safe payload;
- timing distribution;
- behavior across repeated runs.

If no approved threshold exists, Tester MUST report the measurement without inventing PASS/FAIL criteria.

### 6.3 PROBE

Question:

> Is an important technical assumption actually true in the real environment?

PROBE is a bounded engineering experiment.

Its output is evidence for planning/design, not merely a bug verdict.

Examples:

- whether a dynamically exposed tool is actually callable;
- whether a token/identifier remains stable across tool calls;
- whether a value changes across chats, runtime restarts, or target changes;
- whether a browser/runtime capability behaves as assumed;
- whether an integration boundary is actually usable before deeper implementation is built on top of it.

## 7. Readiness model

Tester must distinguish visibility from real usability.

For runtime/tool/integration capabilities, use the following conceptual progression when applicable:

```text
DISCOVERED
    |
    v
REACHABLE
    |
    v
INVOCABLE
    |
    v
FUNCTIONAL
    |
    v
GOAL_MET
```

Definitions:

- `DISCOVERED` — the capability/tool is visible or enumerated;
- `REACHABLE` — the service/endpoint/tool surface can be reached;
- `INVOCABLE` — the required operation can actually be called from the active execution surface;
- `FUNCTIONAL` — the operation produces correct behavior/data on the intended target;
- `GOAL_MET` — the complete checkpoint promise/user flow works.

Earlier stages MUST NOT be treated as proof of later stages.

"Loaded", "registered", "exposed", "READY" status text, or successful startup are not by themselves proof that the checkpoint goal is met.

## 8. Verification vs design evidence

Tester produces two broad classes of useful output.

### Verification evidence

Used to answer whether the current checkpoint goal is satisfied.

Examples:

- assertions;
- test results;
- browser flows;
- screenshots;
- runtime output;
- reproducible failures.

### Design evidence

Used as input to the next phase when architecture depends on real behavior.

Examples:

- identifier/token lifetime;
- readiness timing;
- retry behavior;
- runtime stability;
- actual limits;
- integration behavior across session boundaries.

Design evidence should include:

- experiment setup;
- repeated observations;
- changed variables;
- measured values;
- limitations;
- supported implications;
- confidence based on the number/range of observations.

Tester may state evidence-supported design implications, but it does not silently make product/architecture decisions that belong to Planner/Human.

## 9. Example: CadGPT live CAD capability

Checkpoint goal:

> After a user selects an AutoCAD drawing, ChatGPT can actually operate on that selected drawing through CAD MCP.

Insufficient evidence:

```text
Secure tunnel READY
CAD MCP loaded
CAD tool family discovered
31 proxy tools exposed
cad__cad_list_layers present
```

Those observations prove only partial readiness.

A valid VERIFY path continues until:

```text
drawing detected
  -> drawing selected/bound
  -> CAD MCP usable
  -> cad__cad_list_layers actually invocable
  -> real layer data returned
  -> data belongs to the selected drawing
  -> checkpoint goal satisfied
```

If the proxy tool is discovered but cannot actually be invoked from the active ChatGPT surface, Tester should report something like:

```text
classification: INTEGRATION_NOT_READY
observed:
  - CAD detected
  - drawing bound
  - CAD MCP loaded
  - tool discovered
  - direct invocation unavailable
implication:
  - do not assume tool discovery == tool usability
  - investigate exposure/readiness/invocation layer before changing list_layers logic
```

Tester should not claim that `list_layers` itself is broken without evidence that its implementation is the failing layer.

## 10. Example: CadGPT identity/session probe

A later design may need a stable key for chat-scoped or drawing-scoped state.

Tester should not assume that a value named "session", "token", or "id" has the desired lifetime.

A PROBE can measure:

```text
same tool call sequence
same chat over multiple calls
same chat after delay
new chat
runtime restart
AutoCAD restart
same drawing reopened
different drawing
```

For every candidate identifier/token, record whether it stays stable or changes at each boundary.

Example output:

```text
identifier       same-chat   new-chat   runtime-restart   drawing-change
-----------------------------------------------------------------------
candidate-A      stable      changes    changes           stable
candidate-B      changes     changes    changes           changes
candidate-C      stable      stable     changes           changes
```

That evidence can then become input to Planner/Coder:

- a same-chat-stable/new-chat-changing value may be suitable for chat-scoped state;
- a per-request-changing token is unsuitable as a durable workspace key;
- a drawing-stable identifier may be a candidate for drawing identity.

The selection of the final architecture remains a design/planning decision based on this evidence.

## 11. UI/browser verification

When browser capability is actually usable, Tester may independently verify objective UI behavior such as:

- navigation;
- click/fill/keyboard flows;
- focus behavior;
- visible/hidden states;
- viewport overflow;
- elements being clipped/covered;
- responsive breakpoint behavior;
- console/page errors;
- incorrect or missing assets;
- required user flow completion.

A UI FAIL should include reproducible evidence such as screenshot, snapshot, log, or observed browser-step failure when available.

Subjective judgments or actions that inherently require a person/real device may be routed as `NEEDS_HUMAN`.

Examples:

- subjective visual quality;
- physical-device/touch feel;
- CAPTCHA/2FA;
- irreversible real-world actions;
- behavior that cannot be validly observed in the available environment.

## 12. Verdicts

Tester supports:

- `PASS`
- `FAIL`
- `BLOCKED`
- `NEEDS_HUMAN`

Semantics:

### PASS

All required checkpoint criteria assigned to Tester are verified for the exact target revision/state.

### FAIL

At least one required behavior is verified to be wrong/not ready.

### BLOCKED

Tester cannot reach a valid conclusion because required environment, dependency, capability, or specification is unavailable/broken.

### NEEDS_HUMAN

Automatable verification is complete, but at least one required criterion inherently requires human observation/action.

### Hard rule

`UNVERIFIED != PASS`

Partial execution must not be presented as complete success.

## 13. Report shape

The exact persisted schema may evolve, but the semantic output should support at least:

```json
{
  "checkpoint": "CP2",
  "mode": "VERIFY",
  "target_revision": "<revision>",
  "goal": "<checkpoint goal>",
  "tests": {
    "planned": 9,
    "executed": 9,
    "passed": 7,
    "failed": 1,
    "blocked": 1
  },
  "observations": [],
  "failures": [
    {
      "test": "session_invalid_token",
      "expected": "unauthenticated",
      "actual": "authenticated",
      "classification": "PRODUCT_FAILURE",
      "evidence": []
    }
  ],
  "measurements": {},
  "limitations": [],
  "verdict": "FAIL",
  "next_phase_inputs": []
}
```

Coverage or other metrics may be included when meaningful, but they are optional and MUST NOT substitute for goal verification.

## 14. Relationship to Reviewer

Tester and Reviewer have different responsibilities.

```text
Coder
  |
  | implementation + lightweight self-check
  v
Tester
  |
  | executable behavior evidence / measurements / probes
  v
Reviewer
  |
  | semantic and implementation review against Job/spec
  v
next gate
```

When Tester finds a clear product failure, the normal path is:

```text
Tester FAIL
  -> Coder fixes
  -> Tester RETEST
```

There is no value in forcing full Reviewer work on an implementation that is already proven to fail the current executable checkpoint, unless orchestration explicitly needs Reviewer input for diagnosis or scope decisions.

## 15. Core principles

1. Tester verifies the checkpoint goal, not merely the code that was just written.
2. Tester derives tests independently from spec/goal/acceptance criteria.
3. Tester owns a separate writable test workspace; product source is read-only.
4. Coder self-test is lightweight; Tester verification is deeper and independent.
5. Test results may be outputs **and** inputs to later phases.
6. Important assumptions should be measured/probed when practical instead of guessed.
7. Tool discovery/readiness labels are not proof of actual usability.
8. Evidence is tied to the exact target revision/state.
9. Tester distinguishes product failure, test failure, environment failure, and not-ready integration.
10. UNVERIFIED is never PASS.
11. Checkpoints should be meaningful and sparse enough to avoid orchestration overhead, but early enough to prevent large amounts of code from being built on an unverified foundation.
12. Tester may refine tests within scope, but must not silently expand product scope or make planning decisions on behalf of Planner/Human.

## 16. Current implementation gap

The current GSA Local Tester implementation is substantially narrower than this specification.

At the time this spec was written, the current Tester primarily:

- receives existing verification evidence;
- reads product/project context;
- performs a read-only semantic review;
- submits a structured verdict.

It does not yet fully implement:

- independent test planning;
- dedicated Tester-owned writable workspace;
- test authoring;
- test execution lifecycle;
- adaptive test refinement;
- VERIFY/MEASURE/PROBE modes;
- design-evidence output;
- checkpoint-level accumulated regression;
- explicit readiness progression;
- deep retest lifecycle.

This document defines the intended target behavior. Implementation work should be planned separately and introduced incrementally without making orchestration unnecessarily heavy.
