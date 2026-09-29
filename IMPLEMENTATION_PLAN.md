# GSA Local — Implementation Plan

## 1. Mục tiêu

Xây dựng phiên bản **GSA chạy hoàn toàn local**, sử dụng Ollama trực tiếp và giữ nguyên triết lý Super Agent hiện tại:

- System prompt/harness hiện tại của GSA tiếp tục là **master instruction**.
- Runtime local được code hóa để tự kiểm tra trạng thái, điều phối workflow, active đúng Job Pack và xử lý các loop.
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
 ├─ Milestone Controller
 ├─ Checkpoint Resolver
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
MASTER PROMPT
= định nghĩa cách Agent phải làm việc

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

## 4. System prompt hiện tại của GSA

Không rewrite toàn bộ behavior thành code.

System prompt hiện tại tiếp tục làm **master instruction** cho từng workflow.

Planner prompt chịu trách nhiệm:

```text
inspect project
understand architecture
identify dependencies
avoid drift
design implementation approach
produce implementation plan
```

Reviewer prompt quyết định:

```text
plan có đầy đủ không
logic đúng không
dependency hợp lý không
scope có drift không
```

Coder prompt quyết định:

```text
cách implement
cách sửa code
cách đọc codebase
cách giữ scope
```

CR prompt quyết định:

```text
critical review độc lập
blind spots
requirement mismatch
architecture issue
false PASS
```

Code runtime không thay thế những judgment này.

---

## 5. Runtime code hóa workflow

Những gì có thể kiểm chứng thì không để model tự quản.

Runtime chịu trách nhiệm:

```text
state
transitions
required artifacts
allowed next steps
dependency checks
retry limits
test evidence
milestone activation
jobpack activation
checkpoint recovery
```

Ví dụ Planner gọi `submit_plan()`, runtime không tin ngay mà kiểm:

```text
plan artifact exists?
required sections exist?
plan version current?
blocking state unresolved?
```

Pass mới chuyển Reviewer.

---

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
Fix
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
Fix         → model
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

## 13. Planner CR loop

```text
Planner
 ↕
Reviewer
 ↓ PASS
CR
 ├─ PASS
 │    ↓
 │ PLAN_APPROVED
 │
 └─ REVISE
      ↓
     Fix
      ↓
   Reviewer
      ↓
      CR
```

CR sử dụng fresh context, không nhận toàn bộ reasoning history của Planner.

---

## 14. `PLAN_APPROVED` là boundary

Chỉ khi:

```text
Planner PASS
Reviewer PASS
CR PASS
```

runtime tạo:

```text
PLAN_APPROVED
```

Sau đó mới gọi Job Builder.

---

# JOB BUILDING

## 15. Job Builder Agent

Job Builder không thiết kế implementation.

Nó nhận **APPROVED IMPLEMENTATION PLAN** và chuyển thành execution structure.

Nhiệm vụ:

```text
TODO
Checklist
Job Packs
Milestones
Dependencies
Registration
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
registry.register_plan()
registry.register_milestone()
registry.register_jobpack()
registry.register_todo()
registry.register_checklist()
registry.link_dependency()
```

---

## 21. Registry

Ưu tiên SQLite:

```text
.gsa/state/gsa.db
```

Registry giữ:

```text
plans
milestones
jobpacks
todos
dependencies
checklists
verification states
review states
attempts
events
```

---

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

Cần một checkpoint rất nhỏ:

```json
{
  "sequence": 184,
  "milestone": "M2",
  "jobpack": "JP-07",
  "stage": "reviewer",
  "jobpack_status": "active"
}
```

Checkpoint không phải truth:

```text
Registry = truth
Checkpoint = fast pointer
```

---

## 25. Checkpoint Writer

Chỉ ghi checkpoint ở meaningful transitions:

```text
Job Pack ACTIVE
Coder submitted
Reviewer revise
Reviewer pass
Build started
Test started
Test pass/fail
CR started
CR revise
CR pass
Job Pack DONE
Milestone COMPLETE
next Milestone ACTIVE
```

Không ghi sau mỗi tool call.

---

## 26. Checkpoint Resolver

Khi `gsa` start hoặc workflow cần resume:

```text
read latest checkpoint
        ↓
light validation
        ↓
valid?
 ├─ YES → resume
 └─ NO  → query Registry
           ↓
         repair checkpoint
```

Light validation:

```text
Milestone còn ACTIVE?
Job Pack thuộc đúng Milestone?
Job Pack chưa DONE?
Dependencies satisfied?
Có Job Pack ACTIVE khác không?
Sequence còn current?
```

Core functions:

```text
resolve_latest_checkpoint()
activate_next_jobpack()
```

---

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
Coder/Fix
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

Nếu project chưa có đủ verification, có thể tạo task-specific verification scripts.

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
     Fix
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
 ├─ REVISE → Fix → Reviewer → Verify → CR
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
approved plan
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

## Phase 0 — Audit GSA hiện tại

Audit:

```text
system prompts
agent roles
planner behavior
review behavior
CR behavior
job pack concepts
milestone concepts
existing workflow rules
```

Phân loại:

```text
KEEP AS MASTER INSTRUCTION
MOVE TO RUNTIME
LOCALIZE
REMOVE
```

---

## Phase 1 — CLI + Ollama

Implement:

```text
gsa
cwd workspace
Ollama native client
streaming
/agent
/model
/config
```

---

## Phase 2 — Harness Engine

Implement:

```text
Agent registry
harness loader
tool permission
model resolver
context builder
session model overrides
```

---

## Phase 3 — Workflow Runtime

Implement:

```text
workflow signals
state transitions
validation
handlers
retry limits
```

Planner → Reviewer → CR trước.

---

## Phase 4 — Job Builder

Implement Agent và contracts cho:

```text
TODO
Checklist
Job Pack
Milestone
Dependencies
Registration
```

---

## Phase 5 — Persistent Registry

Implement SQLite:

```text
registry API
validation
atomic transitions
event log
```

Không dùng Markdown làm state.

---

## Phase 6 — Checkpoint system

Implement:

```text
Checkpoint Writer
resolve_latest_checkpoint()
activate_next_jobpack()
checkpoint repair
resume
```

Mục tiêu: `gsa` khởi động lại phải biết ngay:

```text
Milestone nào
Job Pack nào
Gate nào
Agent nào tiếp tục
```

mà không cần đọc lại toàn bộ Markdown để đoán.

---

## Phase 7 — Coder workflow

Implement:

```text
Coder
Reviewer
Fix
Job Pack state
```

Runtime active đúng Job Pack từ Registry.

---

## Phase 8 — Local CI + Verification

Implement:

```text
project stack detection
existing test/build discovery
Local CI Runner
verification evidence
test capability detection
```

---

## Phase 9 — Tester checkpoints

Implement Tester có khả năng chen vào giữa Job Pack khi:

```text
buildable
unit-testable
integration-testable
browser-testable
```

Không ép test khi chưa có giá trị.

---

## Phase 10 — Code CR

Implement:

```text
fresh CR context
independent review
CR → Fix
Fix → Reviewer
Verification
CR again
```

---

## Phase 11 — Milestone execution

Implement:

```text
Milestone lock/unlock
JP eligibility
Milestone verification
Milestone completion
next milestone activation
```

---

## Phase 12 — Hardening

Negative tests:

```text
path escape
state corruption
dependency cycles
duplicate registration
stale checkpoint
crash during transition
multiple active JP
false test PASS
model fake completion
loop exhaustion
```

---

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
CR
↓
PLAN_APPROVED
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
Registry registration
```

Runtime:

```text
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
CR
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
PLANNER
= Implementation Plan

REVIEWER
= close-loop quality review

CR
= independent critical gate

JOB BUILDER
= TODO + Checklist + Job Pack + Milestone + Register

REGISTRY
= execution truth

CHECKPOINT RESOLVER
= fast resume + active-work pointer

MILESTONE CONTROLLER
= unlock/lock work

CODER
= implementation

FIX
= targeted corrections

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
PLAN
 ↓
REVIEW
 ↓
CR
 ↓
JOB BUILD
 ↓
REGISTER
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
CR
 ↓
DONE
 ↓
NEXT JOB PACK
 ↓
NEXT MILESTONE
```

Đây là baseline kiến trúc của **GSA Local**.
