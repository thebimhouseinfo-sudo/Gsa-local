# GSA Local — Implementation Plan

## 1. Mục tiêu

Xây dựng phiên bản **GSA chạy hoàn toàn local**, sử dụng Ollama trực tiếp và giữ nguyên triết lý Super Agent hiện tại:

- **Shared GSA operating rules và các behavior đã chứng minh hiệu quả** tiếp tục là master instruction.
- Mỗi role có **Local Role Contract** riêng; không copy nguyên xi role contract hiện tại khi semantics local đã thay đổi.
- Runtime local được code hóa để tự kiểm tra trạng thái, bind execution vào đúng plan revision, điều phối workflow, active đúng Job Pack và xử lý các loop.
- Model làm reasoning.
- Runtime quản lý process.
- Script/test cung cấp evidence thực.
- Không phụ thuộc GitHub, Codex, OpenCode hay Claude Code.

Cách chạy:

```bash
cd /path/to/project
gsa
```

Thư mục hiện tại chính là workspace.

---

## 2. Kiến trúc tổng thể

```text
USER
 ↓
gsa
 ↓
Project = cwd
 ↓
GSA Runtime
 ├─ Agent/Harness Engine
 ├─ Model Manager
 ├─ Workflow Engine
 ├─ Job Registry
 ├─ Plan Revision Binder
 ├─ Milestone Controller
 ├─ Checkpoint Resolver
 ├─ Execution Lock
 ├─ Verification Controller
 ├─ Local CI Runner
 ├─ Tool Runtime
 └─ Project Boundary
 ↓
Ollama Native API
 ↓
Local Model
```

Nguyên tắc:

```text
SHARED GSA RULES
= nguyên tắc vận hành chung được giữ từ GSA hiện tại

LOCAL ROLE CONTRACT
= định nghĩa cách từng Agent phải làm việc trong GSA Local

MODEL
= reasoning và tạo kết quả

RUNTIME CODE
= enforce workflow

REGISTRY
= trạng thái công việc thật

CHECKPOINT
= pointer nhẹ tới trạng thái mới nhất

TOOLS / CI
= evidence thực

HANDLERS
= quyết định bước tiếp theo
```

---

## 3. Agent = Harness, không phải process AI riêng

Một Agent gồm:

```text
system prompt
+ role
+ allowed tools
+ context policy
+ output contract
+ workflow signals
```

Ví dụ:

```text
Planner harness
      +
qwen38t
      ↓
Ollama
```

Hoặc:

```text
Planner harness
      +
model khác
```

Planner vẫn giữ hành vi Planner vì hành vi nằm trong harness.

---

## 4. Master instruction và Local Role Contract

Không copy nguyên xi từng role contract hiện tại sang local.

Cấu trúc chuẩn:

```text
Shared GSA Operating Rules
        ↓
Local Role Contract
        ↓
Runtime State Contract
        ↓
Model
```

### 4.1 Shared GSA Operating Rules — giữ làm master instruction

Giữ các nguyên tắc đã đúng và đã được kiểm chứng:

```text
source truth trước suy đoán
preserve unrelated user changes
read-before-write
scope discipline
do not route around denied boundaries
discover real build/test commands
verification = evidence
tests alone != goal proof
do not manufacture PASS
minimum sufficient context
fresh review context
```

### 4.2 Local Role Contract — phải localize

Role contract hiện tại chỉ là nguồn để migrate behavior, không phải file được copy nguyên trạng.

Ví dụ Planner hiện tại có trách nhiệm decomposition thành Tasks/Job. GSA Local đã tách trách nhiệm này:

```text
Planner
= Implementation Plan only

Job Builder
= TODO + Checklist + Job Pack + Milestone + Register
```

Vì vậy Local Planner Contract phải bỏ phần Job/Task registration khỏi Planner.

### 4.3 Critic Reviewer / Local CR governance

GSA Local uses the same CR governance invariant:

```text
Human-invoked only
stateless/fresh by contract
durable evidence only
read-only
independent verdict first
exact target binding
```

CR is an independent backstop, not an automatic lifecycle gate.

Normal Planner/Coder workflows use Reviewer automatically where required. CR runs only after an explicit Human invocation.

A plan may expose or recommend a CR checkpoint, but runtime must not dispatch CR autonomously and must not reinterpret Reviewer PASS as authorization to call CR.

If Human invokes CR and CR returns findings, the owning workflow routes those findings through Planner/Internal Fix/Reviewer as appropriate. A second CR pass occurs only if Human invokes CR again.

### 4.4 Internal Fix Harness

Fix là internal repair harness được runtime gọi khi CR/Tester/Reviewer trả findings có thể sửa trong scope.

Fix không cần là user-facing Agent trong MVP.

```text
CR/Tester finding
   ↓
Internal Fix
   ↓
Reviewer
   ↓
Verification nếu applicable
   ↓
CR
```

Code runtime không thay thế judgment của model; nó enforce contracts, state và evidence.

## 5. Runtime code hóa workflow

Những gì có thể kiểm chứng thì không để model tự quản.

Runtime chịu trách nhiệm:

```text
state
transitions
required artifacts
allowed next steps
dependency checks
plan revision/hash binding
atomic registry transitions
retry limits
test evidence
milestone activation
jobpack activation
checkpoint recovery
single-execution lock
```

Ví dụ Planner gọi `submit_plan()`, runtime không tin ngay mà kiểm:

```text
plan artifact exists?
required sections exist?
plan version current?
blocking state unresolved?
```

Reviewer/CR verdict cũng phải bind vào exact plan revision/hash.

Một transition chỉ được commit nếu precondition trong Registry vẫn đúng tại thời điểm write.

Runtime phải reject stale submissions, stale review verdicts và stale Job Builder registrations.

## 6. CLI chỉ có 3 command

Interactive GSA hiện chỉ expose:

```text
/agent
/model
/config
```

Không tạo:

```text
/plan
/code
/cr
/test
/job
/milestone
/status
```

Planner, Coder, CR... là Agent, không phải command.

---

## 7. `/agent`

Dùng để chọn manual entry-point Agent.

Ví dụ:

```text
/agent

General
Planner
Job Builder
Reviewer
Coder
Tester
CR
```

User:

```text
/agent
→ Planner

> Lên implementation plan cho chức năng X.
```

Từ đó workflow tự chạy.

Hoặc:

```text
/agent
→ Coder

> Tiếp tục thực hiện công việc.
```

Runtime sẽ resolve đúng Job Pack thay vì để Coder tự chọn việc.

---

## 8. `/model`

Hot swap model lõi của Agent hiện tại.

Session-only.

Ví dụ preset:

```text
Coder → qwen3.8:27b-mlx
```

Trong session:

```text
/model
→ qwen38t:latest
```

thì Coder dùng model mới trong session hiện tại.

Đóng `gsa` thì override mất. Lần sau Agent quay về model preset trong `/config`.

Override được giữ riêng theo từng Agent. Auto-trigger sang Agent khác không kế thừa model override của Agent hiện tại.

---

## 9. `/config`

Wizard cấu hình persistent.

Wizard tự lấy danh sách từ Ollama tương đương `ollama list`, rồi cho chọn mapping:

```text
General     → model
Planner     → model
Job Builder → model
Reviewer    → model
Coder       → model
Tester      → model
CR          → model
```

Model precedence:

```text
session /model override
        ↓
Agent preset
        ↓
default model
```

---

## 10. Ollama Native

GSA giao tiếp trực tiếp:

```text
/api/chat
/api/tags
/api/show
```

Không dùng compatibility layer của Codex, OpenCode, Claude Code hay OpenAI-compatible wrapper.

Runtime cần support:

```text
streaming
tool calls
multi-turn
model switching
keep_alive
thinking metadata nếu có
```

---

# PLANNING WORKFLOW

## 11. Planner Agent

Planner chỉ có một nhiệm vụ chính:

**Tạo Implementation Plan.**

Planner không:

```text
tạo TODO registry
chia Job Pack
bố trí Milestone
đăng ký execution state
```

Implementation Plan mô tả:

```text
goal
current architecture
required changes
implementation approach
dependencies
sequence
risks
acceptance direction
evidence needs / unknown runtime facts
```

`EvidenceNeed / UnknownRuntimeFact` là first-class planning data, không phải prose tùy ý. Mỗi need phải có identity, câu hỏi cần xác minh, mục đích, required/optional, consumer dự kiến, mode VERIFY/MEASURE/PROBE phù hợp và measurement/acceptance intent. Nó tham gia plan hash/revision để Reviewer/CR duyệt chính contract empirical này. Legacy PlanArtifact không có field này phải deserialize với danh sách rỗng để không phá durable state cũ.

---

## 12. Planner review loop

```text
Planner
   ↓
Reviewer
   ├─ CHANGES_REQUIRED → Planner
   │                     ↓
   │                  Reviewer
   │
   └─ PASS
        ↓
   PLAN_APPROVED eligibility
```

Reviewer is the normal automatic planning quality gate.

CR is not part of this automatic loop. Human may invoke CR independently after a meaningful planning checkpoint.

## 13. Planner review loop + Human-invoked CR backstop

Normal planning loop:

```text
Planner
 ↕
Reviewer
 ↓ PASS
PLAN_APPROVED
```

Optional independent backstop:

```text
Human invokes CR
  ↓
CR reads exact plan revision/hash independently
  ├─ no actionable finding -> durable CR evidence
  └─ finding
       ↓
     Planner/Internal Fix
       ↓
     Reviewer
```

CR is never auto-triggered, stays read-only, binds to the exact plan revision/hash, and does not inherit Planner/Reviewer reasoning history.

If Human wants another CR pass after repair, Human invokes it again.

## 14. `PLAN_APPROVED` là revision-bound boundary

Chỉ khi:

```text
Planner artifact valid
Reviewer PASS on current revision
no unresolved explicit Human Gate
```

runtime tạo:

```text
PLAN_APPROVED
plan_revision = N
plan_hash = <content hash>
```

`plan_revision + plan_hash` trở thành immutable execution binding cho Job Builder.

Nếu Implementation Plan được sửa sau đó:

```text
old execution graph
→ STALE / SUPERSEDED

new plan revision
→ phải Reviewer review lại
→ Human may invoke CR independently if desired
→ Job Builder đăng ký graph mới
```

Không được tiếp tục execute Job Pack sinh từ plan revision cũ.

Sau `PLAN_APPROVED` mới gọi Job Builder.

# JOB BUILDING

## 15. Job Builder Agent

Job Builder không thiết kế implementation.

Nó nhận **APPROVED IMPLEMENTATION PLAN + exact plan_revision + plan_hash** và chuyển thành execution structure.

Nhiệm vụ:

```text
TODO
Checklist
Job Packs
Milestones
Dependencies
Verification hints
Test Checkpoints
VERIFY / MEASURE / PROBE modes
Named empirical evidence outputs
EvidenceRequirement edges
Registration bound to approved plan revision
```

Job Builder phải phân biệt:

```text
verification hint
= gợi ý cách kiểm deterministic cho một Job Pack

TestCheckpointSpec
= boundary có chủ đích do planning quyết định nơi Tester phải chạy

EvidenceRequirement
= dữ liệu quan sát bắt buộc mà work phía sau cần dùng
```

Nếu approved plan nói phase sau cần một runtime fact chưa biết, Job Builder phải tạo checkpoint/evidence dependency phù hợp hoặc trả `PLAN_GAP`. Nó không được tự điền giá trị giả để hoàn tất graph.

Pipeline:

```text
Approved Plan
     ↓
Job Builder
     ↓
TODO
Checklist
Job Packs
Milestones
Dependencies
     ↓
Test Checkpoints
Named Evidence Outputs
Evidence Requirements
     ↓
Register
```

---

## 16. TODO

Job Builder phân rã plan thành các TODO cụ thể.

Ví dụ:

```text
T-01 Implement Ollama client
T-02 Add streaming
T-03 Add model resolver
T-04 Add session override
T-05 Add workflow transitions
```

TODO không phải execution authority. Nó là thành phần để build Job Pack.

---

## 17. Checklist

Mỗi task/job cần checklist rõ ràng.

Ví dụ:

```text
T-01 Ollama client

[ ] /api/chat
[ ] streaming supported
[ ] errors normalized
[ ] timeout handled
[ ] response mapped correctly
```

Checklist giúp Reviewer và runtime biết required work.

---

## 18. Job Pack

Job Builder gom TODO thành đơn vị công việc meaningful nhưng vừa đủ nhỏ.

Ví dụ:

```text
JP-01 — Ollama Foundation

T-01 Ollama client
T-02 Streaming
```

Một Job Pack phải có:

```text
id
goal
milestone
TODOs
checklist
dependencies
required inputs
expected outputs
acceptance
verification hints
required evidence inputs (nếu có)
checkpoint boundary refs (nếu có)
```

`required evidence inputs` không phải prose tự do. Khi chúng trỏ tới Tester evidence, Registry/Controller phải resolve exact named output + provenance + target compatibility trước khi Job Pack được dùng.

---

## 19. Milestone

Job Builder bố trí Job Pack vào Milestone.

Ví dụ:

```text
M1 — Runtime Foundation
 ├─ JP-01 Ollama client
 └─ JP-02 Model manager

M2 — Agent Runtime
 ├─ JP-03 Harness loader
 └─ JP-04 Context builder

M3 — Workflow Engine
 ├─ JP-05 Planner loop
 └─ JP-06 Coding loop
```

Job Pack không được chạy ngoài Milestone đang active.

---

# REGISTRY / CHECKPOINT

## 20. Registration phải bằng script/runtime

Markdown không phải execution state.

Nguyên tắc cứng:

```text
.md
= description

Registry
= truth

Runtime transition
= authority
```

Job Builder không chỉ ghi `[x] registered` vào Markdown. Nó phải gọi Registry API:

```text
registry.register_plan_binding(plan_revision, plan_hash)
registry.register_milestone(...)
registry.register_jobpack(...)
registry.register_todo(...)
registry.register_checklist(...)
registry.link_dependency(...)
```

Registration phải được thực hiện trong transaction.

Preconditions:

```text
PLAN_APPROVED still current?
plan_revision matches?
plan_hash matches?
no newer approved revision exists?
execution graph for this revision not already superseded?
```

Nếu một precondition thay đổi giữa lúc Job Builder reasoning và lúc commit, transaction phải reject và Job Builder phải reload source truth.

## 21. Registry

Ưu tiên SQLite:

```text
.gsa/state/gsa.db
```

Registry giữ:

```text
plans
approved_plan_revision/hash
execution_graph_version
milestones
jobpacks
todos
dependencies
checklists
verification states
review states
attempts
events
latest_checkpoint
execution_lease
```

Registry là source of truth duy nhất cho execution state.

Không dùng một JSON checkpoint độc lập làm authority.

## 22. Registry validation

Khi Job Builder đăng ký:

```text
Job Builder
    ↓
Registry API
    ↓
Validator
```

Kiểm:

```text
ID unique?
Milestone exists?
Job Pack belongs to valid milestone?
Dependencies exist?
Dependency cycle?
Orphan Job Pack?
Empty Milestone?
Missing acceptance?
Missing checklist?
Invalid ordering?
Approved plan revision/hash still current?
Existing execution graph superseded?
More than one ACTIVE Job Pack?
```

Chỉ PASS mới commit state.

---

## 23. Job Pack và Milestone state

Job Pack:

```text
PENDING
READY
ACTIVE
REVIEW
VERIFY
CR
BLOCKED
DONE
```

Milestone:

```text
LOCKED
ACTIVE
VERIFY
COMPLETE
```

Runtime mới có quyền đổi trạng thái.

---

## 24. Lightweight latest checkpoint

Không scan toàn database/event history mỗi lần.

Checkpoint là **một lightweight indexed row trong Registry**, không phải state file độc lập.

Ví dụ logical record:

```json
{
  "sequence": 184,
  "plan_revision": 3,
  "milestone": "M2",
  "jobpack": "JP-07",
  "stage": "reviewer",
  "jobpack_status": "active"
}
```

Nguyên tắc:

```text
Registry tables = truth
latest_checkpoint row = fast pointer trong cùng DB
```

Checkpoint được cập nhật trong **cùng transaction** với state transition tạo ra nó.

## 25. Checkpoint Writer

Chỉ cập nhật checkpoint ở meaningful transitions:

```text
Job Pack ACTIVE
Coder submitted
Reviewer revise
Reviewer pass
Build started
Test started
Test pass/fail
Human-invoked CR started (when applicable)
Human-invoked CR finding/closed (when applicable)
Human-invoked CR evidence persisted (when applicable)
Job Pack DONE
Milestone COMPLETE
next Milestone ACTIVE
```

Không ghi sau mỗi tool call.

State transition và checkpoint update phải atomic:

```text
BEGIN
  validate current state/version
  write transition
  append event
  update latest_checkpoint
COMMIT
```

Crash giữa transaction không được để state và checkpoint lệch nhau.

## 26. Checkpoint Resolver + Execution Lock

Khi `gsa` start hoặc workflow cần resume:

```text
acquire project execution lease
        ↓
read latest_checkpoint indexed row
        ↓
light validation
        ↓
valid?
 ├─ YES → resume
 └─ NO  → reconstruct từ Registry
           ↓
         repair checkpoint transactionally
```

Light validation:

```text
plan_revision/hash còn current?
Milestone còn ACTIVE?
Job Pack thuộc đúng Milestone?
Job Pack chưa DONE?
Dependencies satisfied?
Có Job Pack ACTIVE khác không?
Checkpoint sequence khớp transition sequence?
```

Core functions:

```text
acquire_execution_lease()
resolve_latest_checkpoint()
activate_next_jobpack()
release_execution_lease()
```

### Single-process execution lease

MVP enforce:

```text
one project
→ one active execution lease
→ max one ACTIVE Job Pack
```

Nếu user mở terminal thứ hai và chạy `gsa` cùng project, process thứ hai không được tự activate thêm Job Pack.

Lease phải có owner/process identity và stale-lease recovery policy để crash không khóa project vĩnh viễn.

## 27. Không được nhảy Milestone

Nếu M2 chưa complete:

```text
M2 ACTIVE
M3 LOCKED
```

Dù Job Pack của M3 có vẻ đã thỏa dependency, runtime vẫn không cho chạy.

Chỉ khi:

```text
all required JP in M2 DONE
+
Milestone verification satisfied
```

mới:

```text
M2 COMPLETE
M3 ACTIVE
```

---

# CODING WORKFLOW

## 28. Coder không tự chọn việc

Khi user chọn:

```text
/agent
→ Coder
```

runtime chạy trước:

```text
Checkpoint Resolver
 ↓
Milestone Controller
 ↓
Job Pack Resolver
```

rồi inject **ACTIVE JOB PACK** cho Coder.

---

## 29. Context của Coder

Coder chỉ nhận context cần cho Job Pack:

```text
Coder master instruction
+
project summary
+
approved plan revision/hash
+
current milestone
+
active Job Pack
+
TODO
+
checklist
+
dependencies
+
acceptance
+
relevant files
+
previous findings nếu có
```

Không dump toàn bộ plan/history.

---

## 30. Coding inner loop

Trong một Job Pack:

```text
Coder
  ↓
Reviewer
  ├─ REVISE → Coder
  └─ PASS
```

Reviewer kiểm:

```text
requirement coverage
logic
scope
diff
architecture compliance
unnecessary changes
error handling
```

---

# VERIFICATION / LOCAL CI / TESTER

## 31. Test Checkpoint là planning artifact

Tester không tự xuất hiện chỉ vì code vừa Reviewer PASS hoặc vì Verification Controller phát hiện project có test.

Checkpoint placement là dữ liệu do Planner/Job Builder chuẩn bị trong Execution Graph.

Ví dụ:

```text
JP-08
  Dev A
   ↓
  Reviewer PASS

  Dev B
   ↓
  Reviewer PASS
   ↓
  TEST CHECKPOINT CP-01
      modes: VERIFY + MEASURE
      outputs:
        - api_ready_state
        - startup_latency_ms

  Dev C
   ↓
  requires CP-01.startup_latency_ms
```

Nếu không có declared checkpoint thì Reviewer PASS chỉ kết thúc vòng review hiện tại; Tester không tự chạy.

Checkpoint không chỉ bind vào một Job Pack. Topology phải hỗ trợ checkpoint sau một tập Job Pack đã được review, trước một Job Pack consumer, hoặc tại milestone integration gate. Mỗi prerequisite có `PrerequisiteState` rõ ràng. Phase 10 có thể dùng exact `REVIEW_PASS` target và có thể đọc trạng thái `DONE` đã tồn tại, nhưng không được tự tạo `DONE/COMPLETE`. Chỉ khi toàn bộ prerequisite đạt trạng thái khai báo checkpoint mới trở thành DUE.

---

## 32. Verification Controller đổi vai trò

Verification Controller vẫn giữ giá trị của Phase 9:

```text
discover build/test/lint/typecheck capability
run deterministic approved commands
capture exit code/stdout/stderr/timing
prevent model-created false PASS
```

Nhưng Verification Controller **không còn authority quyết định Tester placement**.

Nó là một deterministic capability/evidence primitive dùng cho:

```text
Coder lightweight self-check
declared VERIFY checkpoint
declared MEASURE/PROBE adapter khi phù hợp
final verification
```

Các capability như:

```text
BUILD_ONLY
UNIT_TESTABLE
INTEGRATION_TESTABLE
BROWSER_TESTABLE
FULL_TESTABLE
```

chỉ mô tả cái gì runtime có thể kiểm chứng, không có nghĩa Tester phải chạy ngay.

---

## 33. Build-only self-check

Nếu coding checkpoint chỉ build được:

```text
Coder
 ↓
lightweight deterministic build/self-check
 ↓
Reviewer
```

Không cần Tester nếu Execution Graph không có Test Checkpoint tại boundary đó.

Build evidence vẫn được lưu để Reviewer/CR/Tester dùng khi cần.

---

## 34. Declared Test Checkpoint

Khi execution đạt một checkpoint đã khai báo:

```text
reviewed target
   ↓
CHECKPOINT DUE
   ↓
Tester
```

Checkpoint có thể gồm:

```text
VERIFY
MEASURE
PROBE
```

VERIFY hỏi:

```text
sản phẩm có đáp ứng goal/acceptance không?
```

MEASURE hỏi:

```text
giá trị thực tế là bao nhiêu?
behavior phân bố thế nào?
```

PROBE hỏi:

```text
assumption kiến trúc này có đúng trên runtime thật không?
```

MEASURE/PROBE không tự tạo PASS/FAIL nếu plan không định nghĩa threshold.

---

## 35. Tester Agent

Tester là checkpoint subsystem độc lập.

Tester có quyền:

```text
read product source
read approved plan / checkpoint contract
write Tester-owned tests/fixtures/artifacts
execute bounded tests/probes
collect measurements
produce OBSERVED evidence
report implications/limitations
```

Tester không có quyền:

```text
write product source
write product config/deployment config
choose checkpoint placement
invent missing acceptance criteria
invent measurement thresholds
make final product architecture decisions
```

Tester-owned workspace:

```text
.gsa/tester/<graph>/<checkpoint>/<attempt>/
```

Coder/Internal Fix không được sửa workspace này để ép test PASS.

---

## 36. Empirical evidence là first-class dependency

Một checkpoint có thể không chỉ gate chất lượng mà còn cung cấp dữ liệu thật cho phase sau.

Ví dụ:

```text
PROBE CP-SESSION
  observe:
    token stable in same chat?
    token changes in new chat?
    runtime restart effect?
    drawing change effect?
```

Output không phải một câu model tự kết luận.

Registry phải lưu structured observations:

```text
dimension
sample index
changed boundary/event
target revision/change_set
observable runtime/tool identity
observed value
unit
limitations
evidence refs
```

Job Pack sau có thể khai báo:

```text
requires:
  CP-SESSION.session_binding_behavior
```

Runtime chỉ inject evidence tương thích có provenance:

```text
OBSERVED
```

`IMPLICATION` và `UNRESOLVED` không được dùng thay giá trị đo bắt buộc.

Mỗi reusable empirical output phải có `EvidenceApplicability` mô tả những dimension làm nó còn hợp lệ hoặc hết hạn: product/config/runtime/environment identity, dependency fingerprint, boundary conditions và revalidation policy. Applicability phải dùng finite declarative matcher allowlist và được runtime đánh giá; model prose không có quyền override. Exact revision binding vẫn là evidence identity; applicability quyết định evidence có thể reuse sau thay đổi nào.

Nếu thiếu required OBSERVED evidence:

```text
BLOCKED / PLAN_GAP
```

không được tự bịa biến để tiếp tục.

---

## 37. Tester failure và retest

Nếu Tester xác định:

```text
PRODUCT_FAILURE
```

flow là:

```text
Tester FAIL
 ↓
Coder/Internal Fix
 ↓
lightweight self-check
 ↓
Reviewer
 ↓ PASS
Tester RETEST same checkpoint
```

Tester không sửa product source.

Nếu lỗi thuộc test:

```text
TEST_FAILURE
```

Tester có thể sửa test artifact trong workspace riêng rồi chạy lại trong giới hạn attempt.

Nếu lỗi thuộc environment/capability:

```text
ENVIRONMENT_FAILURE
NOT_READY
INTEGRATION_NOT_READY
```

không được chuyển thành product FAIL/PASS giả.

---

## 38. Local CI evidence

Local CI vẫn là deterministic evidence source.

Runtime ghi:

```text
command / adapter id
argv
cwd policy
exit_code
stdout/stderr
duration
timeout
artifact/version
target binding
```

Không tin model nói "Tests passed."

States deterministic:

```text
TEST_PASS
TEST_FAIL
TEST_NOT_APPLICABLE
TEST_BLOCKED
```

`TEST_NOT_APPLICABLE != PASS`.

`TEST_BLOCKED != PASS`.

Tester mode outcomes là contract riêng:

```text
VERIFY  -> PASS / FAIL / BLOCKED / NEEDS_HUMAN
MEASURE -> COMPLETE / BLOCKED / NEEDS_HUMAN
PROBE   -> COMPLETE / BLOCKED / NEEDS_HUMAN
```

UNVERIFIED không bao giờ trở thành PASS.

---

# FINAL JOB PACK GATES

## 39. Job Pack final gate

Khi tất cả TODO/checklist hoàn thành:

```text
Job Pack implementation complete
       ↓
Reviewer PASS
       ↓
Final verification if applicable
       ↓
CR
```

CR chỉ chạy khi Job Pack đủ trưởng thành để independent review.

---

## 40. Code CR loop

```text
CR
 ├─ PASS
 │    ↓
 │ JP_DONE
 │
 └─ REVISE
      ↓
     Internal Fix
      ↓
   Reviewer
      ↓
Verification if applicable
      ↓
      CR
```

Không cho `Fix → CR` mà bỏ qua Reviewer/verification.

---

## 41. Job Pack completion

Human-invoked CR evidence does not directly mutate Job Pack state.

Runtime thực hiện:

```text
jobpack.complete()
```

Registry kiểm:

```text
TODO complete?
Checklist complete?
Reviewer pass?
Required verification satisfied?
If an explicit Human CR gate exists for this work, is that Human-owned gate satisfied?
```

Sau đó:

```text
JP → DONE
```

và ghi checkpoint.

---

# MILESTONE EXECUTION

## 42. Milestone Controller

Khi JP DONE:

```text
Milestone Controller
 ↓
remaining eligible Job Pack?
```

Nếu có:

```text
activate next Job Pack
```

Nếu không:

```text
check milestone completion
```

---

## 43. Milestone verification

Một Milestone có thể đạt trạng thái integration-testable dù từng Job Pack đã pass riêng.

Ví dụ:

```text
M1
├─ Ollama client DONE
├─ Model manager DONE
└─ Session runtime DONE
```

Khi đủ cả ba:

```text
Milestone VERIFY
 ↓
build complete executable
 ↓
integration test
 ↓
Tester nếu cần
```

Chỉ sau PASS:

```text
M1 COMPLETE
```

---

## 44. Next Milestone

```text
M1 COMPLETE
 ↓
M2 LOCKED → ACTIVE
 ↓
Checkpoint Writer
 ↓
resolve first eligible Job Pack
 ↓
JP ACTIVE
```

Không để model tự chuyển Milestone.

---

## 45. Full execution loop

```text
APPROVED PLAN
      ↓
JOB BUILDER
      ↓
TODO
CHECKLIST
JOB PACK
MILESTONE
DEPENDENCIES
      ↓
REGISTER
      ↓
Registry Validation
      ↓
Checkpoint
      ↓
Milestone Controller
      ↓
ACTIVE JOB PACK
      ↓
Coder
      ↕
Reviewer
      ↓
Verification checkpoint?
 ┌────┴─────┐
 │          │
NO         YES
 │          ↓
 │      Build/Test
 │          │
 │      FAIL → Fix loop
 │          │
 └──────── PASS
      ↓
Continue Job Pack
      ↓
Final Reviewer
      ↓
Final verification
      ↓
CR
 ├─ REVISE → Internal Fix → Reviewer → Verify → CR
 └─ PASS
      ↓
JP DONE
      ↓
Checkpoint Writer
      ↓
Milestone Controller
 ├─ next JP
 └─ milestone verification
          ↓
      milestone complete
          ↓
      next milestone
```

---

# RUNTIME SAFETY / CONTEXT

## 46. Project boundary

`cwd` là project root.

Runtime enforce bằng code:

```text
../
absolute path escape
symlink escape
git -C outside
shell redirect outside
subshell escape
```

Không dựa vào prompt.

---

## 47. Tools

Core:

```text
filesystem
search
process
git-local
workflow
registry
verification
```

Tool permission theo Agent.

Planner:

```text
mostly read/search
```

Job Builder:

```text
read plan
registry registration
```

Reviewer/CR:

```text
read/search/diff
```

Coder/Fix:

```text
read/write/patch/process
```

Tester:

```text
process/browser/test tools
```

---

## 48. Delete policy

Delete thông thường:

```text
move to Trash
```

Hard delete chỉ khi user yêu cầu rõ.

---

## 49. Context isolation

CR luôn nhận clean packet:

```text
original requirement
approved plan
current Job Pack
relevant code/diff
verification evidence
current acceptance criteria
```

Không nhận producer reasoning history.

Reviewer có thể có context gần hơn để sửa nhanh.

---

## 50. Persistent state

Persistent:

```text
approved plan + revision/hash
execution graph version
execution registry
milestones
jobpacks
todos
checklists
dependencies
verification evidence
review findings
CR findings
event log
latest checkpoint
execution lease
```

Không persist:

```text
/model session overrides
hidden chain-of-thought
temporary model context
```

---

## 51. Loop limits

Auto loop phải có limit.

Ví dụ:

```text
Planner ↔ Reviewer       max N
Human-invoked Plan CR repair cycle (when invoked)  max N

Coder ↔ Reviewer         max N
Test fix loops           max N
Human-invoked Code CR repair cycle (when invoked) max N
```

Nếu không hội tụ:

```text
PAUSED
```

trả quyền lại user.

---

# IMPLEMENTATION PHASES

## Phase 0 — Audit và migration map từ GSA hiện tại

Audit:

```text
shared operating rules
Planner contract
Reviewer contract
Coder contract
Tester contract
Critic Reviewer contract
Job/Task concepts
review evidence rules
verification rules
```

Mỗi rule được phân loại:

```text
KEEP AS SHARED MASTER RULE
LOCALIZE INTO ROLE CONTRACT
MOVE TO RUNTIME
REMOVE
```

Deliverable bắt buộc là một migration matrix; không copy nguyên role contract hiện tại.

---

## Phase 1 — CLI + Ollama Native

Implement:

```text
gsa
cwd workspace
Ollama detection
/api/tags
/api/show
/api/chat
streaming
/agent
/model
/config
```

Acceptance: cùng một harness chạy được với model preset và session override khác nhau.

---

## Phase 2 — Harness Engine + Local Role Contracts

Implement:

```text
Shared GSA Rules loader
Local Role Contract loader
Agent registry
tool permission
model resolver
context builder
session model overrides
internal Fix harness
```

Acceptance: role semantics local không còn phụ thuộc các rule web/human-only không phù hợp.

---

## Phase 3 — Registry + Atomic Transition Core

Implement SQLite trước orchestration cao cấp:

```text
schema
plan revision/hash records
execution graph version
registry API
atomic transition API
event log
compare-and-set/version checks
single ACTIVE Job Pack invariant
```

Acceptance: stale transition và duplicate activation bị reject bằng code.

---

## Phase 4 — Checkpoint + Execution Lease

Implement:

```text
latest_checkpoint indexed row
transactional checkpoint writer
resolve_latest_checkpoint()
checkpoint repair
execution lease
stale lease recovery
resume
```

Acceptance: crash/restart không tạo thêm ACTIVE Job Pack và terminal thứ hai không chiếm cùng execution.

---

## Phase 5 — Planner → Reviewer (+ Human-invoked CR backstop)

Core planning contract:

```text
Planner Implementation Plan only
plan revision/hash
Reviewer fresh planning review
Internal Fix loop
PLAN_APPROVED binding
Human-invoked CR backstop
```

Tester architecture extension:

```text
PlanArtifact EvidenceNeed / UnknownRuntimeFact
submit_plan structured schema
evidence need validation
evidence need included in plan hash/revision
prior OBSERVED evidence catalog for Planner
PLAN_GAP when required runtime fact is unresolved
```

Planner still does not build Job Pack/Checkpoint directly. It persists empirical needs clearly enough that Job Builder does not re-infer intent from prose.

Acceptance:

```text
Planner ↔ Reviewer
        ↓ PASS
PLAN_APPROVED(revision/hash)
```

A Human-invoked CR may review the exact planned/approved revision independently, but CR is not an automatic prerequisite for PLAN_APPROVED.

Stale Reviewer/CR evidence cannot validate a newer revision.

## Phase 6 — Job Builder + Registration

Core đã implement:

```text
TODO
Checklist
Job Pack
Milestone
Dependencies
verification hints
plan-bound execution graph
transactional registration
graph validation
supersede old graph
```

Tester architecture extension:

```text
EvidenceNeed from approved PlanArtifact
TestCheckpointSpec
CheckpointBoundaryKind
prerequisite Job Pack set
VERIFY / MEASURE / PROBE
named evidence outputs
EvidenceRequirement
EvidenceApplicability
experiment dimensions
downstream evidence consumers
submit_execution_graph schema update
```

Acceptance mới:
- Job Builder không thay đổi Implementation Plan;
- không register graph cho stale plan revision;
- không tự bịa checkpoint, threshold hoặc measured value;
- evidence need từ approved plan phải được chuyển thành explicit checkpoint/evidence dependency hoặc trả PLAN_GAP.

---

## Phase 7 — Milestone + Job Pack Controller

Core đã implement:

```text
Milestone lock/unlock
JP eligibility
dependency resolution
activate_next_jobpack()
active-work resolver
milestone completion preconditions
```

Tester architecture extension:

```text
declared checkpoint boundary resolution
checkpoint DUE/RUNNING/SATISFIED/BLOCKED/NEEDS_HUMAN
multi-prerequisite checkpoint resolution
EvidenceRequirement + EvidenceApplicability resolution
OBSERVED evidence injection into ActiveWork
transactional latest_checkpoint update
lease-bound checkpoint/attempt resume
```

Acceptance mới:
- không jump milestone;
- đúng một Job Pack ACTIVE;
- không jump qua required Test Checkpoint;
- không activate consumer work khi required empirical evidence còn thiếu/stale/incompatible.

---

## Phase 8 — Coder ↔ Reviewer Workflow

Core giữ nguyên:

```text
Coder scoped to active Job Pack
lightweight self-check
Reviewer read-only review
Internal Fix routing
TODO/checklist state
goal recheck
diff/scope evidence
```

Tester architecture extension:
- ActiveWork/Coder context nhận required OBSERVED evidence;
- thiếu required evidence thì block thay vì Coder tự đoán;
- Coder/Internal Fix bị chặn khỏi Tester-owned workspace;
- repair sau Tester PRODUCT_FAILURE vẫn phải quay qua Reviewer trước RETEST.

Acceptance: Coder không tự chọn Job Pack, không tự bịa missing runtime inputs, và Reviewer findings luôn bind vào đúng artifact/revision.

---

## Phase 9 — Verification Controller + Local CI

Core giữ nguyên:

```text
stack/config detection
discover existing build/test/lint/typecheck commands
BUILD_ONLY / UNIT / INTEGRATION / BROWSER capability
Local CI Runner
evidence records
TEST_PASS / FAIL / NOT_APPLICABLE / BLOCKED
task-specific test-script review
```

Semantic change từ Tester architecture:
- Verification Controller không quyết định Tester placement;
- capability detection chỉ mô tả verification surface hiện có;
- deterministic verification là primitive dùng bởi Coder self-check, declared Test Checkpoints và final verification;
- Test Checkpoint placement chỉ đến từ approved Execution Graph.

Acceptance: model claim không thể tạo PASS nếu deterministic evidence không tồn tại, và capability discovery không tự tạo undeclared Tester checkpoint.

---

## Phase 10 — Tester Checkpoint Subsystem

Tester is an independent checkpoint subsystem, not part of the normal Coder↔Reviewer loop.

Normal coding remains:

```text
Coder
  -> lightweight self-check
  -> Reviewer
  -> Internal Fix when required
  -> Reviewer PASS
```

Tester is invoked only when execution reaches a **Test Checkpoint declared in the Milestone/Execution Graph**. Checkpoints are intentionally sparse and meaningful.

Each checkpoint may use one or more Tester modes:

- `VERIFY` — verify that the current product state satisfies the checkpoint goal;
- `MEASURE` — collect real observed values/behavior without inventing thresholds;
- `PROBE` — test an architectural/runtime assumption before later work depends on it.

Tester owns:

- independent test planning from goal/spec/acceptance;
- a dedicated writable Tester workspace for tests, fixtures, artifacts and reports;
- bounded test/experiment execution;
- result classification and diagnosis;
- exact-target evidence persistence;
- retest after product fixes;
- accumulated regression evidence for later checkpoints.

Tester product-source access remains read-only. Coder must not edit Tester-owned tests merely to force a pass.

Checkpoint outputs are first-class graph artifacts. Verification evidence may gate continuation. Empirical/design evidence may become an explicit input/dependency of later Planner/Coder phases, preventing downstream work from inventing identifiers, timing constants, readiness assumptions, limits or other runtime facts.

When Tester reports a product failure:

```text
Tester FAIL
  -> Coder/Internal Fix
  -> lightweight self-check
  -> Reviewer PASS
  -> Tester RETEST same checkpoint on the new exact target
```

Tester must distinguish `PRODUCT_FAILURE`, `TEST_FAILURE`, `ENVIRONMENT_FAILURE`, `NOT_READY/INTEGRATION_NOT_READY`, and `SPEC_GAP`.

Verdicts are `PASS`, `FAIL`, `BLOCKED`, and `NEEDS_HUMAN`. `UNVERIFIED != PASS`.

Implementation must keep Tester checkpoint logic separate from Phase 11 Job Pack completion and from any optional Human-invoked CR backstop.

Crash/restart contract:

```text
checkpoint state transition
+ event
+ latest_checkpoint pointer
= one transaction
```

Execution lease phải bind checkpoint attempt để restart/terminal thứ hai không tạo duplicate attempt. Mỗi executable Tester step phải có replay-safety class như `OBSERVE_ONLY / IDEMPOTENT / NON_IDEMPOTENT`. Sau crash, uncertain NON_IDEMPOTENT step không được auto-replay; runtime phải BLOCK/NEEDS_HUMAN hoặc dùng recovery adapter rõ ràng.

Material `SPEC_GAP` hoặc architectural uncertainty không được Tester tự giải bằng cách sửa topology:

```text
Tester SPEC_GAP
  -> persist evidence
  -> pause current graph
  -> Planner/Human
  -> new PlanArtifact revision
  -> Reviewer
  -> Human may invoke CR independently if required
  -> Job Builder
  -> superseding ExecutionGraph
```

In-scope test adaptation bên trong checkpoint hiện tại vẫn được Tester thực hiện mà không cần replan.

---

## Phase 11 — Job Pack Completion + Human-invoked CR backstop

Implement:

```text
required Reviewer / Verification / Tester gates
  ↓
jobpack completion eligibility
```

When Human invokes CR at a meaningful boundary:

```text
fresh CR packet
exact Job Pack/revision binding
independent read-only review
CR finding
  -> Internal Fix
  -> Reviewer
  -> Verification/Tester as applicable
```

CR is never auto-dispatched and never substitutes for normal completion gates. If Human wants CR to re-check repaired work, Human invokes CR again.

Acceptance: Job Pack completion depends on its declared gates; CR evidence is a Human-invoked backstop unless Human explicitly establishes a CR gate for that work.

## Phase 12 — Milestone Verification + Full Resume

Implement:

```text
milestone-level integration verification
Milestone VERIFY → COMPLETE
next Milestone activation
checkpoint resume across all workflow stages
```

Acceptance: restart tại bất kỳ stage nào resume đúng Milestone/Job Pack/Agent/gate.

---

## Resume / Recovery hardening derived from real interrupted sessions

Full evidence and rationale: WORKFLOW_LESSONS_LEARNED.md.

Phase 12 must not be considered complete merely because latest_checkpoint and lease exist. Full resume additionally requires a shared recovery protocol:

    durable workflow state
      + live source revision
      + open/terminal Run lineage
      + canonical gate evidence
      -> ResumeDecision
      -> exactly one next safe action

Required architecture additions:
- Online Project resume remains only a Project lifecycle operation; interrupted execution needs a separate open-Run/recovery resolver.
- Local PlanningWorkflow and CodingWorkflow must resolve existing durable workflow state before calling begin_*; begin_* is initialization, not resume.
- Run/source binding must separate input target from result target.
- source-ahead interrupted work must be classified before adoption.
- gate evidence must have canonical runtime-maintained pointers instead of requiring the model to rewire ref arrays.
- required UNVERIFIED/BLOCKED verification cannot produce terminal PASS.
- Handoff availability and next-Task activation are separate states; Human-bounded task/phase transitions default to explicit start.
- stale planning/runtime evidence needs structured supersession/invalidation semantics.
- restart tests must cover every persisted workflow stage plus source/durable divergence.

Acceptance extension for Phase 12:
- restart cannot reset a partially completed workflow;
- restart cannot create a duplicate Run/attempt or replay uncertain non-idempotent work;
- restart cannot bind review/evidence to the wrong source revision;
- restart cannot require conversation history to discover the execution point;
- restart cannot silently enter the next Task/phase when activation requires explicit Human start;
- recovery from source-ahead state is explicit and auditable.

## Phase 13 — Hardening

Negative tests:

```text
path escape
symlink escape
shell redirect escape
git -C outside
state corruption
dependency cycles
duplicate registration
stale plan registration
stale review verdict
stale checkpoint
crash during transaction
dual-terminal execution
multiple ACTIVE JP
false test PASS
weak/fake model-created test
model fake completion
loop exhaustion
```

# MVP DEFINITION OF DONE

User:

```bash
cd project
gsa
```

Sau đó:

```text
/agent
→ Planner

> Lên implementation plan cho X.
```

GSA tự:

```text
Planner
↔ Reviewer
↓ PASS
PLAN_APPROVED(revision/hash)

Human may invoke CR independently at a meaningful checkpoint
```

Sau đó:

```text
Job Builder
↓
TODO
Checklist
Job Packs
Milestones
Dependencies
↓
Registry registration bound to approved plan revision/hash
```

Runtime:

```text
execution lease
↓
latest checkpoint
↓
active milestone
↓
active Job Pack
```

Coder workflow:

```text
Coder
↔ Reviewer
↓
Build/Test khi thực sự testable
↓
continue coding
↓
Final verification
↓
runtime validates all declared gates

Human may invoke CR independently at a meaningful checkpoint
↓
JP DONE
```

Sau đó tự:

```text
next Job Pack
↓
...
↓
Milestone verification
↓
Milestone COMPLETE
↓
next Milestone
```

Nếu terminal đóng giữa chừng, chạy lại:

```bash
gsa
```

thì lightweight checkpoint resolver phải khôi phục đúng:

```text
Milestone
Job Pack
stage
next Agent
```

mà không cần đọc lại toàn bộ Markdown để đoán.

---

## 44.5 Evidence-aware next work

Trước khi activate Job Pack hoặc phase tiếp theo, controller phải resolve:

```text
normal dependencies
+
required Test Checkpoints
+
required EvidenceRequirement
```

Nếu một work item yêu cầu runtime evidence:

```text
OBSERVED evidence available + compatible
    ↓
inject exact values/refs into context

missing / stale / incompatible
    ↓
BLOCKED / PLAN_GAP
```

Không được thay required evidence bằng suy đoán model.

---

# BOUNDARY KIẾN TRÚC CUỐI

```text
SHARED GSA RULES
= master behavior common to all local roles

LOCAL ROLE CONTRACT
= localized role-specific behavior

PLANNER
= Implementation Plan + revision/hash

REVIEWER
= close-loop quality review

LOCAL CR
= Human-invoked independent critical backstop, fresh/stateless, read-only

JOB BUILDER
= TODO + Checklist + Job Pack + Milestone + Register against approved plan revision/hash

REGISTRY
= execution truth

CHECKPOINT RESOLVER
= fast resume from transactional latest_checkpoint

EXECUTION LOCK
= one project → one active execution lease

MILESTONE CONTROLLER
= unlock/lock work

CODER
= implementation

INTERNAL FIX
= targeted repair harness, not required as user-facing Agent

VERIFICATION CONTROLLER
= decide when testing has value

LOCAL CI
= deterministic evidence

TESTER
= test/behavior reasoning when actually useful

RUNTIME
= enforce everything
```

Core flow:

```text
PLAN(revision/hash)
 ↓
REVIEW
 ↓
PLAN_APPROVED
 ↓
JOB BUILD
 ↓
REGISTER TRANSACTIONALLY
 ↓
EXECUTION LEASE
 ↓
CHECKPOINT
 ↓
MILESTONE
 ↓
JOB PACK
 ↓
CODE
 ↓
REVIEW
 ↓
BUILD / TEST WHEN POSSIBLE
 ↓
DONE
 ↓
NEXT JOB PACK
 ↓
NEXT MILESTONE
```

Đây là baseline kiến trúc của **GSA Local**.


# IMMEDIATE REBASELINE BOUNDARY — BEFORE T-ORCHESTRATION

This boundary applies now. It is an operational use of the Human Rebaseline/Salvage contract before the future runtime automation exists.

Current state:
- J-177F is IN_PROGRESS on an older durable planning revision.
- T-EXECUTION has reached a reviewed + CI-passing implementation boundary.
- T-ORCHESTRATION has not started.
- Human has materially changed workflow architecture, resume semantics, CR governance and future UI/Tester architecture.

Required action before any additional coding:
1. Freeze J-177F execution at the current boundary; do not start T-ORCHESTRATION under the stale topology.
2. Capture live SourceTargetSet and all durable review/CI/Tester evidence relevant to work completed through T-EXECUTION.
3. Classify completed work and evidence through Rebaseline/Salvage; preserve source by default.
4. Treat T-EXECUTION and its prerequisites as candidates for adoption, not automatic discard and not automatic authority under the new graph.
5. Create a replacement remaining-work Job/graph for T-ORCHESTRATION onward under the migrated contracts.
6. Record adopted_from_runs/change_sets/evidence and required reverification in the replacement plan.
7. Reviewer must PASS the replacement plan before coding resumes.
8. CR remains Human-invoked only.

Current control plane has no SUSPENDED_BY_REBASELINE state. Do not bypass this by manually editing J-177F JSON; the Human stop and reviewed replacement plan are the temporary governance bridge.
# UI / RECOVERY ARCHITECTURE REBASELINE — FUTURE PHASES

Authoritative design inputs for this replan are read-only:
- WORKFLOW_LESSONS_LEARNED.md
- GSA_LOCAL_UI_TESTER_TARGET_ARCHITECTURE.md

These phases are added to the total architecture plan and execute only through the replacement/future Jobs created after the immediate rebaseline. They do not retroactively expand or resume the frozen J-177F topology.

## Phase 14 — Recovery Orchestrator + WorkCursor

Goal: make restart/recovery a deterministic runtime operation rather than model inference.

Required:
- Project resolution before Job/Run resolution;
- authoritative WorkCursor / ResumeDescriptor;
- START vs RESUME vs RECOVER distinction;
- plan/hash/graph-version binding;
- multi-repo SourceTargetSet;
- canonical current gate evidence;
- crash-consistent transition idempotency;
- parent/child Run reconciliation with explicit continuation/correction/review/retest/recovery/supersession lineage;
- machine-readable GateDiagnostic for every blocked completion/resume gate;
- workflow-level replay policy for Run/Verification/Handoff/source/Job/CI/deploy/browser/external-UI-provider actions;
- early deterministic-verification readiness state;
- source mutation write-ahead identity;
- monotonic attempt budgets;
- durable Human EXPLICIT_START boundary;
- lease owner nonce + fencing token;
- durable-state schema migration rules.

Exit:
- a fresh process resolves exactly one next safe action from durable state + live source;
- no manual Run/target/ref JSON repair is required;
- unknown lineage fails closed.

## Phase 15 — Human Rebaseline / Salvage + Bounded Integration

Goal: handle intentional Human architecture changes without either blindly resuming old authority or discarding all useful work.

Required:
- HUMAN_REBASELINE_REQUIRED / REBASELINING;
- freeze/snapshot affected work;
- architecture epoch/rebaseline identity;
- classify and salvage prior ChangeSets/commits/evidence;
- dependency-impact propagation;
- revised plan/graph based on live salvaged baseline;
- semantic old-Run supersession;
- AcceptedIntegrationBaseline;
- READY_FOR_INTEGRATION vs AUTHORIZED_TO_MERGE;
- partial multi-repo integration state.

Exit:
- previous valid work is explicitly adopted/adapted/superseded/reverted;
- no blanket reset;
- no old-epoch mutation;
- integration happens at bounded reviewed/verified boundaries rather than after unbounded commit accumulation.

## Migration coverage gate — workflow lessons

| Contract family | Plan owner |
|---|---|
| Resume identity / exact target / no chat-history dependency | Phase 14 WorkCursor/ResumeDescriptor |
| Transactional finalization / idempotency | Phase 14 transition identity |
| Parent-child Run reconciliation | Phase 14 Run lineage |
| Gate mismatch diagnostics / canonical evidence | Phase 14 GateDiagnostic + current evidence pointer |
| Workflow replay + verification readiness | Phase 14 replay/readiness contract |
| Project-before-Job resolution | Phase 14 resolver |
| Lease fencing | Phase 14 lease contract |
| Mutation WAL | Phase 14 mutation identity |
| Schema migration | Phase 14 durable-state versioning |
| Human architecture interruption | Immediate rebaseline bridge + Phase 15 automation |
| Bounded integration | Phase 15 AcceptedIntegrationBaseline |

Coverage is complete only when each family has runtime acceptance tests or an explicit Human governance gate; prose reference alone is insufficient.
## UI execution policy status — UNDER REVIEW / CR REQUIRED

> **Status: UNDER REVIEW**
>
> This Local UI execution policy is provisional architecture. It remains marked **UNDER REVIEW** until a **Human-invoked Critic Reviewer (CR)** independently reviews this exact plan revision, adjusts the policy where needed, and explicitly clears the mark.
>
> Normal Reviewer PASS does **not** remove this mark. Planner, Reviewer, Coder, Tester, Job Builder, or runtime may not silently convert it to accepted architecture. Only a Human-invoked CR review may authorize removing **UNDER REVIEW**.
>
> Until CR clears it, Phases 16–20 are planning guidance for the Local UI architecture but must not be treated as an irreversible mandatory workflow contract.

### Environment-aware UI execution premise

Shared UI principles remain common across GSA variants:

```text
Planner owns functional/UX intent
Designer owns visual interpretation when design work is needed
UI Coder owns implementation
Tester verifies independently
Human owns subjective visual acceptance
```

Execution strategy is allowed to differ by runtime capability.

```text
GSA Local
  stronger Vercel agent-browser development/inspection loop
  -> source-render-browser refinement can be used more directly

GSA Online
  more limited browser feedback/runtime interaction
  -> a heavier pre-code visual-design workflow may be justified more often
```

This difference is **not workflow drift** when both variants preserve the same authority, evidence and Human-review contracts. Planner must treat available runtime/browser capability as a first-class routing input.

The same product/UI request may therefore legitimately choose different execution modes in Online and Local when the observable tool capability differs.

If Online browser capability improves later, its reliance on separate pre-code design tooling may decrease without changing the shared UI governance model. Local does not require a fixed design-authoring application.

## Phase 16 — UI Planning + Visual Authority

Goal: preserve UI as a distinct engineering workflow without depending on a separate design-authoring application.

Required:
- Designer role plus UX Coder/UI Coder specialization;
- preserve UI_FIRST / UX_FIRST as product workflow classification;
- visual_authority_source = EXISTING_APPROVED_SHELL | EXISTING_DESIGN_SYSTEM | DESIGNER_SPEC | HUMAN_DIRECTION;
- any existing shell is preserved by default;
- substantial visual improvement is allowed directly in source;
- discarding/rebuilding/replacing an existing shell requires explicit Human approval plus Rebaseline/Salvage before destructive source mutation;
- greenfield and rebuild work still execute in runnable source;
- Designer supplies unresolved material visual direction/specification before affected source mutation; later in-loop Designer input is limited to non-material refinement;
- Human retains subjective visual authority.

Exit:
- Planner can define UI work without UI Coder inventing material visual decisions and without introducing a mockup/source dual authority.

## Phase 17 — Vercel agent-browser Shared UI Runtime

Goal: use one browser technology for UI development observation and independent Tester verification without conflating their authority.

Required:
- standalone Local PROBE of launch/open/wait/snapshot/interact/re-snapshot/viewport/screenshot/diff/console/error/cleanup capabilities;
- separate UI Coder development session and Tester verification session;
- UI Coder browser observations never satisfy Tester PASS;
- Tester-owned evidence workspace;
- explicit screenshot/diff paths for Human;
- exact target/viewport/browser-operation applicability;
- PRODUCT_FAILURE / TEST_FAILURE / ENVIRONMENT_FAILURE / INTEGRATION_NOT_READY / SPEC_GAP / PLAN_GAP / NEEDS_HUMAN classification.

Exit:
- required agent-browser operations are proven available for a future UI Coder development session and independent Tester verification session; Phase 18 owns the actual UI Coder workflow.

## Phase 18 — UI Coder Browser-Driven Workflow

Goal: make UI Coder a dedicated source/render/observe/refine workflow rather than ordinary code generation.

UI Coder-owned state model:

    UI_GROUND
      -> UI_IMPLEMENT
      -> UI_RENDER
      -> UI_INSPECT
      -> UI_REFINE
           ↺ UI_RENDER
      -> UI_SELF_CHECK
      -> UI_HANDOFF_READY

Post-self-check orchestration:

    UI_HANDOFF_READY
      -> REVIEW_PENDING        [owner: Orchestrator]
      -> REVIEWING             [owner: Reviewer]
      -> REVIEW_PASS
      -> TEST_PENDING?         [owner: Orchestrator]
      -> TESTING?              [owner: Tester]
      -> HUMAN_REVIEW_PENDING? [owner: Orchestrator/Human gate]
      -> UI_ACCEPTED / SHELL_READY [terminalized by Orchestrator from declared evidence]

Required:
- runnable source is the implementation/prototype truth;
- Vercel agent-browser drives UI Coder observation/refinement;
- UI Coder workspace stores briefs/references/screenshots/comparisons/assets/prompts/tokens/notes/prototype material;
- UI workspace drafts/prototypes are non-runtime and may not be imported/bundled/served by the product; promoted assets move through canonical product asset paths with provenance/approval;
- compile/test success alone does not prove UI completion;
- material product/UX/shell-architecture changes exit the refine loop and route to Planner/Human;
- resume reconstructs exact UI stage + workspace refs;
- every durable UI stage records stage_owner plus producing Run/evidence identity;
- UI Coder cannot transition Reviewer/Tester/Human/terminal stages; role findings route through Orchestrator to a new bounded UI Coder correction/refine Run.

Exit:
- UI Coder can survive interruption and continue the exact visual implementation loop without chat-history reconstruction.

## Phase 19 — UI Acceptance + Shell Workflow

Normal bounded UI change:

    Planner/visual authority
      -> UI Coder source/browser loop
      -> UI_SELF_CHECK
      -> Reviewer
      -> Tester independent agent-browser
      -> Human review if required
      -> UI_ACCEPTED

Greenfield or Human-approved shell rebuild:

    Planner
      -> Designer/Human visual direction where needed
      -> UI Coder builds real shell in source
      -> agent-browser render/inspect/refine loop
      -> representative content pilot
      -> Reviewer
      -> Tester
      -> Human shell checkpoint where required
      -> SHELL_READY

Required:
- existing shell cannot be discarded without Human approval and completed Rebaseline/Salvage of affected work;
- no separate mockup authority exists;
- representative content/responsive stress is used before broad scale-out when shell risk warrants it;
- ordinary UI work need not create a SHELL_READY gate;
- SHELL_READY requires runnable-source evidence and may become an AcceptedIntegrationBaseline.

Exit:
- UI completion is proven against the product that will actually run.

## Phase 20 — UX_FIRST UI Update Packs

Goal: accumulate UX-derived visual needs without either redesigning after every feature or leaving temporary UI permanent.

Required:
- durable UIRequirement records;
- Planner groups coherent requirements into UI Update Packs;
- Designer resolves material visual direction where current authority is insufficient;
- UI Coder implements directly in source through the browser-driven workflow;
- Reviewer + Tester validate the implemented target;
- Human shell-rebuild approval is required only when the pack truly discards/replaces existing shell architecture.

Exit:
- UX_FIRST work converges into maintainable production UI without a parallel design artifact.

## Phase 21 — UI Capability Providers + Skills

Goal: grow UI Coder capability modularly without coupling the workflow to any single vendor/tool.

Optional provider families:
- icon libraries;
- font libraries;
- component libraries;
- open/stock asset sources;
- image generation;
- SVG/vector tooling;
- asset optimization.

Optional skills:
- responsive/layout;
- accessibility;
- design tokens;
- framework-specific UI implementation;
- visual-regression interpretation;
- asset integration.

Contracts:
- source + agent-browser remains sufficient for the core UI workflow;
- provider/skill availability is discovered, never assumed;
- missing optional provider does not block UI work unless the approved contract explicitly requires that resource type;
- durable imported/generated assets record source/provider/version identity where relevant;
- capability extensions never replace Reviewer/Tester/Human authority.

## Phase 22 — UI Workflow Pilots

Required:
- brownfield existing-shell browser-loop pilot;
- simple greenfield real-source shell pilot;
- genuine Human-approved rebuild or controlled rebuild fixture;
- UI Coder/Tester session isolation test;
- Designer-without-fixed-design-tool test;
- restart/resume at every UI Coder state;
- core-no-provider baseline test;
- after provider abstraction exists, provider fallback/absence test;
- UX_FIRST UI Update Pack pilot;
- bounded integration validation.

Exit:
- UI workflow is calibrated from real Local evidence before becoming mandatory across Projects.

## UI phase dependency gates

```text
Immediate operational rebaseline
  -> replacement remaining-work Job
  -> Phase 14 Recovery foundations
  -> Phase 15 automated Rebaseline/Integration
  -> Phase 16 UI visual-authority contract
  -> Phase 17 agent-browser shared runtime + PROBE
  -> Phase 18 UI Coder browser-driven workflow
  -> Phase 19 UI acceptance/shell workflow
  -> Phase 20 UX_FIRST Update Packs
  -> Phase 22 pilots

Phase 18
  -> Phase 21 optional capability providers/skills
```

Cross-cutting gates:
- source + agent-browser is the core Local UI path;
- Designer may be required for unresolved visual intent but no fixed design application is required;
- existing shell rebuild/replacement requires explicit Human approval;
- UI Coder browser evidence never substitutes for Tester OBSERVED verification;
- optional icon/font/asset/component/skill providers never become hidden hard dependencies.
