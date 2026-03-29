# Rust Design Principles For Jean

If Jean is going to be AI-implemented but human-designed, the codebase should be shaped so the compiler can enforce as much of the architecture as possible.

The main rule is: encode runtime invariants in types, not comments.

## 1. Make Illegal States Unrepresentable

Do not model agent or runtime state with loose structs full of `Option`s and string flags. Prefer enums and phase-specific types.

Good:

```rust
enum AgentState {
    Created,
    Queued,
    Running(RunningState),
    WaitingOnTool(ToolWait),
    WaitingOnAgent(AgentWait),
    Suspended,
    Completed(CompletionInfo),
    Failed(FailureInfo),
    Killed(KillInfo),
}
```

Bad:

```rust
struct Agent {
    status: String,
    waiting_on_tool: Option<String>,
    error: Option<String>,
    completed_at: Option<DateTime<Utc>>,
}
```

AI tends to generate "flexible" data models that drift into ambiguity. Avoid that.

## 2. Separate Control-Plane Types From Persistence Types

Have distinct types for:

- in-memory runtime state
- IPC protocol messages
- event log records
- storage snapshots

Do not reuse one struct everywhere.

A good split is:

- `jean-core::protocol::*`
- `jean-core::event::*`
- `jean-core::state::*`
- `jean-core::snapshot::*`

## 3. Prefer Append-Only Events Over Mutable Global Truth

For a runtime, a safer source of truth is often:

- current state = reduced view of prior events

That means:

- persist events first
- derive projections and snapshots second
- make replay a first-class workflow

This gives:

- auditability
- resumability
- debuggability
- easier testing

## 4. Model Capabilities Explicitly

Do not scatter permission booleans through the code.

Bad:

```rust
struct Agent {
    can_run_shell: bool,
    can_write_files: bool,
    can_use_network: bool,
}
```

Better:

```rust
enum Capability {
    ReadFile,
    WriteFile,
    RunCommand(CommandPolicy),
    Network(NetworkPolicy),
    Git(GitPolicy),
}
```

Even better: capability tokens attached to tool requests, validated centrally.

## 5. Keep Async Boundaries Narrow And Obvious

Rust async gets messy fast, and AI-written Rust is especially prone to spreading async everywhere.

Use async only at natural boundaries:

- IPC server
- model calls
- tool subprocess I/O
- event streaming
- storage if truly needed

Keep coordination logic, reducers, validators, policy checks, and state machines synchronous whenever possible.

## 6. Centralize State Transitions

Do not let random modules mutate agent state directly.

Use explicit transition functions:

```rust
fn apply_event(state: &mut AgentState, event: &AgentEvent) -> Result<()>;
fn validate_command(state: &AgentState, cmd: &AgentCommand) -> Result<ValidatedCommand>;
```

This gives one place to reason about lifecycle correctness and makes AI edits safer.

## 7. Prefer Small Enums And Newtypes Over Raw Strings

Any ID or role that matters should get a type:

```rust
struct AgentId(Uuid);
struct RunId(Uuid);
struct ArtifactId(Uuid);
```

Likewise for paths, task ownership, tool call IDs, and approval IDs when they carry meaning. This reduces class-mixing bugs.

## 8. Minimize Shared Mutable State

If something must be shared:

- share an append-only event sink
- share immutable config
- share handles, not business objects

Avoid giant `Arc<Mutex<AppState>>` designs. They are easy for AI to generate and hard to keep coherent.

Prefer actor ownership:

- each subsystem owns its own state
- cross-subsystem interaction happens via commands and events

## 9. Use Traits Sparingly And Only Where Substitution Is Real

AI often over-abstracts Rust with too many generic traits too early.

Prefer concrete types first.

Introduce traits only for true seams:

- model provider
- event store
- artifact store
- tool executor
- approval backend

If there is only one implementation and no real benefit from mocking, keep it concrete.

## 10. Design Modules Around Authority Boundaries

A good Rust module split for Jean is not "files by topic"; it is "files by who is allowed to decide what."

For example:

- `policy` decides whether an action is allowed
- `tools` executes allowed actions
- `agent` decides what to request
- `daemon` supervises lifecycle
- `memory` stores facts, events, and artifacts

Do not let these bleed into each other.

## 11. Prefer Typed Builders At Subsystem Boundaries

For complex requests, use validated builders or constructors instead of public structs with many fields.

Example:

```rust
let req = ToolRequest::builder(agent_id, ToolKind::RunCommand)
    .cwd(cwd)
    .timeout(Duration::from_secs(30))
    .reason("run tests after patch")
    .build()?;
```

This prevents half-baked requests from being created by AI-generated code.

## 12. Use Exhaustive Matching Aggressively

Lean into Rust's `match` checking. For agent runtimes, this is one of the best defenses against silent logic gaps.

When a new state, event, or tool kind is added, the compiler should force updates everywhere relevant.

## 13. Treat Serialization As A Compatibility Boundary

IPC and persisted events should have versioning discipline from day one:

- stable wire enums
- explicit schema evolution plan
- avoid `untagged` serde tricks
- prefer tagged enums
- do not expose internal structs directly

AI will happily "just add a field" unless the boundary is clearly designed.

## 14. Keep Tool Execution In A Very Small Trusted Core

Anything touching:

- filesystem writes
- subprocess spawning
- env inheritance
- network
- git mutations

should live behind a narrow, heavily reviewed module. This is where the least AI creativity and the most explicit checks are wanted.

## 15. Optimize Tests For Invariants, Not Surface Behavior

For Jean, the highest-value tests are:

- state transition tests
- event replay equivalence
- policy enforcement tests
- tool sandbox tests
- cancellation and supervision tests
- IPC roundtrip and schema tests

Do not rely mostly on end-to-end transcript tests. They are useful, but too fuzzy on their own.

## Concrete Rule Of Thumb

For each major subsystem, ask:

- What are the valid states?
- What events can change those states?
- Who owns mutation?
- What invariants should the compiler help enforce?
- What must be persisted?
- What is the stable external boundary?

If those answers are visible in the Rust types, the language is doing its job.

If not, the codebase is likely relying too much on convention and too little on the compiler.
