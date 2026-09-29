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

### 4.3 Local CR khác Critic Reviewer hiện tại

Critic Reviewer hiện tại của GSA là:

```text
human-invoked only
stateless
durable evidence only
read-only
independent verdict first
```

GSA Local giữ các thuộc tính:

```text
stateless / fresh context
read-only review
independent verdict
exact target binding
evidence-based
```

nhưng **bỏ human-only dispatch** cho local workflow.

Local CR là auto gate sau Reviewer PASS.

Không được dùng nguyên human-only CR harness rồi để runtime bypass rule đó.

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
```

---

## 12. Planner review loop

```text
Planner
   ↓
Reviewer
   ├─ REVISE → Planner
   │             ↓
   │          Reviewer
   │
   └─ PASS
        ↓
        CR
```

Reviewer là vòng review gần.

CR chỉ chạy sau Reviewer PASS.

---

## 13. Planner Local CR loop

```text
Planner
 ↕
Reviewer
 ↓ PASS
Local CR
 ├─ PASS
 │    ↓
 │ PLAN_APPROVED
 │
 └─ REVISE
      ↓
     Internal Fix
      ↓
   Reviewer
      ↓
   Local CR
```

Local CR:

- auto-trigger sau Reviewer PASS;
- dùng fresh/stateless review context;
- bind verdict vào exact `plan_revision + plan_hash`;
- read-only đối với plan artifact đang review;
- hình thành verdict độc lập;
- không inherit Planner reasoning history;
- không dùng human-only gate của Critic Reviewer hiện tại.

Nếu Local CR yêu cầu sửa, runtime tạo repair context riêng cho Internal Fix rồi bắt buộc quay lại Reviewer trước khi CR chạy lại.

## 14. `PLAN_APPROVED` là revision-bound boundary

Chỉ khi:

```text
Planner artifact valid
Reviewer PASS on current revision
Local CR PASS on current revision
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
→ phải review lại
→ CR lại
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
Registration bound to approved plan revision
```

Pipeline:

```text
Approved Plan
     ↓
Job Builder
     ↓
TODO
     ↓
Checklist
     ↓
Job Packs
     ↓
Milestones
     ↓
Dependencies
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
```

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
Local CR started
Local CR revise
Local CR pass
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

## 31. Verification không chỉ nằm cuối Job Pack

Trong quá trình dev, một function hoặc nhóm function có thể đạt mức:

```text
buildable
testable
integration-testable
browser-testable
```

Khi đó Tester có thể nhảy vào ngay.

Không cần đợi Job Pack hoàn tất toàn bộ.

---

## 32. Verification Controller

Sau meaningful development checkpoint:

```text
Coder
 ↓
Reviewer PASS
 ↓
Verification Controller
```

Controller đánh giá:

```text
NOT_READY
BUILD_ONLY
UNIT_TESTABLE
INTEGRATION_TESTABLE
BROWSER_TESTABLE
FULL_TESTABLE
```

---

## 33. Build-only checkpoint

Nếu mới chỉ build được:

```text
BUILD_ONLY
 ↓
Local CI Runner
 ↓
build
```

Không cần gọi Tester Agent nếu deterministic build đã đủ.

---

## 34. Testable checkpoint

Nếu thật sự test được:

```text
Coder
 ↓
Reviewer
 ↓
Build
 ↓
Tester
```

Nếu fail:

```text
Tester FAIL
 ↓
Coder/Internal Fix
 ↓
Reviewer
 ↓
Build/Test again
```

Nếu pass:

```text
resume current Job Pack
```

Coder tiếp tục phần còn lại.

---

## 35. Tester Agent

Tester chỉ tham gia khi reasoning hoặc interaction mang lại giá trị.

Tester source implementation là **read-only**. Tester không sửa code. Khi phát hiện failure, Tester ghi evidence và route về Coder/Internal Fix.

Ví dụ:

```text
browser flow
runtime interaction
UI behavior
multi-step CLI behavior
semantic verification
```

Không dùng model để hỏi build có pass không nếu exit code đã trả lời được.

---

## 36. Local CI

GitHub có CI infrastructure sẵn. GSA Local phải có Local CI Runner.

Local CI tìm trước các script của project:

```text
npm test
npm run build
npm run lint
cargo test
pytest
go test
...
```

Nếu project chưa có đủ verification, Coder có thể đề xuất task-specific verification scripts.

Script do model tạo **không tự động trở thành trusted evidence**. Reviewer/Tester phải kiểm mục tiêu và phạm vi của script; Local CI Runner chỉ ghi evidence từ execution thật.

---

## 37. Local CI evidence

Không tin model nói "Tests passed."

Runtime ghi:

```text
command
exit_code
stdout/stderr
timestamp
artifact/version
```

Chỉ evidence thật mới được coi là test/build pass.

Test states:

```text
TEST_PASS
TEST_FAIL
TEST_NOT_APPLICABLE
TEST_BLOCKED
```

`TEST_NOT_APPLICABLE` không phải PASS.

`TEST_BLOCKED` phải được lưu như limitation.

---

## 38. Multiple checkpoints trong một Job Pack

Ví dụ:

```text
JP-08

Dev A
 ↓
Reviewer

Dev B
 ↓
Reviewer
 ↓
Build
 ↓
Test PASS

Dev C
 ↓
Reviewer
 ↓
Integration Test PASS

Dev D
 ↓
Reviewer

Final JP Verification
```

Tester xuất hiện khi test bắt đầu có ý nghĩa.

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

CR PASS chưa trực tiếp sửa Markdown.

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
CR pass?
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
Plan CR fixes            max N

Coder ↔ Reviewer         max N
Test fix loops           max N
Code CR fixes            max N
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

## Phase 5 — Planner → Reviewer → Local CR

Implement:

```text
Planner Implementation Plan only
plan revision/hash
Reviewer fresh planning review
Local CR stateless auto gate
Internal Fix loop
PLAN_APPROVED binding
```

Acceptance:

```text
Planner ↔ Reviewer
        ↓ PASS
      Local CR
        ↓ PASS
PLAN_APPROVED(revision/hash)
```

Stale verdict không thể approve revision mới.

---

## Phase 6 — Job Builder + Registration

Implement:

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

Acceptance: Job Builder không thay đổi Implementation Plan và không register graph cho stale plan revision.

---

## Phase 7 — Milestone + Job Pack Controller

Implement:

```text
Milestone lock/unlock
JP eligibility
dependency resolution
activate_next_jobpack()
active-work resolver
milestone completion preconditions
```

Acceptance: không jump milestone; đúng một Job Pack ACTIVE.

---

## Phase 8 — Coder ↔ Reviewer Workflow

Implement:

```text
Coder scoped to active Job Pack
Reviewer read-only review
Internal Fix routing
TODO/checklist state
goal recheck
diff/scope evidence
```

Acceptance: Coder không tự chọn Job Pack và Reviewer findings luôn bind vào đúng artifact/revision.

---

## Phase 9 — Verification Controller + Local CI

Implement:

```text
stack/config detection
discover existing build/test/lint/typecheck commands
BUILD_ONLY / UNIT / INTEGRATION / BROWSER capability
Local CI Runner
evidence records
TEST_PASS / FAIL / NOT_APPLICABLE / BLOCKED
task-specific test-script review
```

Acceptance: model claim không thể tạo PASS nếu deterministic evidence không tồn tại.

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

Implementation must keep this phase separate from Phase 11 Local CR / Job Pack completion.

---

## Phase 11 — Code Local CR + Job Pack Completion

Implement:

```text
fresh CR packet
exact Job Pack/revision binding
independent read-only review
CR → Internal Fix
Internal Fix → Reviewer
Verification if applicable
CR again
jobpack.complete()
```

Acceptance: CR PASS một mình chưa thể DONE nếu checklist/verification/reviewer preconditions chưa đạt.

---

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
↓
Local CR
↓
PLAN_APPROVED(revision/hash)
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
Local CR
↓
runtime validates all gates
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
= auto independent critical gate, fresh/stateless, read-only

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
LOCAL CR
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
LOCAL CR
 ↓
DONE
 ↓
NEXT JOB PACK
 ↓
NEXT MILESTONE
```

Đây là baseline kiến trúc của **GSA Local**.
