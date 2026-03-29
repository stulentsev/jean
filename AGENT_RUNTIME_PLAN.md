# Jean Agent Runtime Plan

## Goal

Jean should become a state-of-the-art local agent harness built around a long-lived local runtime, not a chat-first TUI. The focus is multi-agent execution on one machine: spawning, killing, signaling, messaging, memory, shared memory, coordination, and guardrails.

Clients should not talk to the LLM directly. They should talk to a local daemon over IPC. The daemon owns orchestration, policy, memory, tools, and model access.

## Core Shape

Preferred top-level components:

- `jean-daemon`: long-lived supervisor/runtime
- `jean-cli`: shell-oriented control client
- `jean-core`: shared types, IDs, protocol, errors
- `jean-agent`: agent loop and orchestration logic
- `jean-tools`: tool registry and execution wrappers
- `jean-memory`: run store, artifact store, durable project memory
- `jean-policy`: sandbox, approvals, path/env/process policy
- `jean-eval`: regression tests and harness benchmarks

The daemon should own:

- agent registry
- session state
- tool execution
- audit log
- memory stores
- event bus
- model access
- policy enforcement

## IPC

Use Unix domain sockets first. JSON messages are sufficient for v1. If throughput or serialization costs become a real problem later, switch to a binary protocol.

Protocol families:

- `Command`: spawn, kill, signal, attach, approve, deny, list, inspect
- `Event`: agent_started, token, tool_requested, tool_started, tool_finished, state_changed, artifact_created, warning, error
- `Query`: get_run, get_agent, get_logs, get_memory, get_artifact
- `Reply`: ok, error, stream_opened, snapshot

The most useful split is:

- request/response RPC for control operations
- append-only event stream for everything interesting

This gives replay, observability, and resumability a clean foundation.

## Agent Model

Each agent should be an actor with:

- inbox
- local scratchpad
- capability token set
- working directory
- resource budget
- parent pointer
- child set
- current task
- lifecycle state

Suggested state machine:

1. `Created`
2. `Queued`
3. `Running`
4. `WaitingOnTool`
5. `WaitingOnAgent`
6. `Suspended`
7. `Completed`
8. `Failed`
9. `Killed`

Signals:

- `cancel`
- `pause`
- `resume`
- `nudge`
- `escalate`
- `shutdown`

Agents should not be treated as "just prompts". They are supervised runtime entities with explicit state, budgets, and capabilities.

## Supervision

Use a supervisor tree.

Root run supervisor:

- owns the top-level task
- spawns planner/worker/verifier agents
- enforces per-run budgets and cleanup

Parent agent responsibilities:

- child spawn
- child cancellation
- message routing
- result acceptance/rejection
- timeout handling

Restart policy:

- tool failures are usually retryable
- model failures should retry with backoff
- repeated failures should bubble to the parent
- killed agents are terminal unless explicitly respawned

## Memory

Memory should be separated by scope.

Stores:

- `RunStore`: all events, state snapshots, decisions
- `AgentScratch`: private short-lived notes
- `TaskSharedMemory`: explicit shared blackboard for sibling agents
- `ProjectMemory`: durable facts about the repo, user, and project
- `ArtifactStore`: files, patches, test outputs, logs, search results

Rules:

- nothing writes to shared memory without attribution
- shared memory entries are typed and timestamped
- project memory is curated, not a raw transcript dump

Good first shared-memory primitives:

- `Fact`
- `Hypothesis`
- `PlanStep`
- `Claim`
- `Evidence`
- `PatchRef`
- `TestResult`

## Coordination

Start with structured roles, not free-form swarms.

Recommended v1 patterns:

- planner -> executor -> verifier
- owner -> delegated workers
- proposer -> critic -> decider

Coordination primitives:

- direct message
- broadcast event
- task lease
- artifact handoff
- claim/evidence posting to a blackboard

Avoid voting and consensus as the default interaction model. Ownership plus verification is a better default than democracy between agents.

## Tools And Guardrails

Tools should be daemon-managed, never raw model-controlled.

Tool classes:

- read-only file/search
- patch/apply edits
- shell command
- git
- test/build
- network
- browser

Each tool invocation should carry:

- requesting agent
- declared purpose
- target paths/processes
- timeout
- output cap
- approval mode
- audit record

Sandbox v1:

- cwd sandbox
- path allowlist/denylist
- env filtering
- subprocess timeout
- stdout/stderr truncation
- destructive command elevation
- optional per-agent git worktree

This is enough to start safely without building a full container runtime.

## Model Loop

The agent loop should be event-driven:

1. Read inbox and relevant memory.
2. Assemble a prompt from state, policies, tools, and context.
3. Call the model.
4. Emit tokens and events.
5. If a tool is requested, validate it against policy, then execute or require approval.
6. Write artifacts and results.
7. Continue or terminate.

Prompt assembly should be centralized in the daemon, with layers:

- global system policy
- runtime/tool policy
- agent role prompt
- task context
- scoped memory
- recent transcript/events

## V1 Build Order

1. `jean-daemon` with run registry and event log
2. IPC control client
3. single-agent runtime with one read-only tool
4. tool framework, approvals, and audit
5. spawn/kill/signal and supervisor tree
6. shared blackboard and artifact store
7. planner/executor/verifier pattern
8. patch/apply and test/build tools
9. eval harness
10. richer CLI/TUI only if still needed

## Concrete Crate Layout

- `/crates/jean-core`
- `/crates/jean-daemon`
- `/crates/jean-cli`
- `/crates/jean-agent`
- `/crates/jean-tools`
- `/crates/jean-memory`
- `/crates/jean-policy`
- `/crates/jean-eval`

## Notes

- The existing Ratatui prototype should be treated as disposable unless a specific part is still useful.
- The current chat-first message model is not the right core abstraction for the new system.
- The product should be centered on the daemon/runtime, not the interface.
