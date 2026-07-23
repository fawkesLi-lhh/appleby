# Appleby Project Rules

## Document Roles

- `APPLEBY.md`: project-wide rules only. Keep it short and direct.
- `FEATURE.md`: decisions about which functions the system keeps, removes, or has not approved.
- `docs/QA/`: concrete questions, user answers, decisions, and reasons. Each file is named only by date: `docs/QA/YYYY-MM-DD.md`. One daily file may contain multiple questions. Record entries as `YYYY-MM-DD-Q1`, `YYYY-MM-DD-A1`, `YYYY-MM-DD-Q2`, `YYYY-MM-DD-A2`, and so on. Do not put general rules or feature lists there.

## Rule Priority

When rules conflict, follow this order.

### 1. Feature Selection

- The production system keeps only features marked `Required` or `Approved` in `FEATURE.md`.
- Every production module, behavior, and direct dependency must implement an approved feature or be necessary to implement it correctly.
- Anything without this mapping should be deleted.
- New features must be decided in `FEATURE.md` before implementation.
- Existing code, tests, compatibility, or possible future use are not sufficient reasons to keep a feature.

### 2. Information Completeness

- Important input, output, errors, and API-call information must have a traceable destination.
- A destination may be `context.jsonl`, tracing logs, user feedback, or a returned error.
- The system must not silently discard failure information or replace it with normal data.
- If information from a fallible operation is intentionally discarded, a code comment must state why that information is not needed.
- Defaults and fallbacks are allowed only when they are explicitly equivalent to the intended semantics.

### 3. Single Implementation

- Each approved capability, business rule, protocol rule, and safety check has one authoritative implementation.
- For one capability, Appleby directly selects one implementation library.
- Do not keep parallel old/new paths for the same function.
- Record concrete implementation choices, rejected alternatives, and reasons in the current day's `docs/QA/YYYY-MM-DD.md`.
- Before adding production code, first identify the feature, dependency, or implementation path that the new code replaces.
- Prefer changes that add required behavior and tests while reducing total production code.

## Project Ownership

- Provider SDK code belongs only in provider adapters. Core runtime uses Appleby-owned types.
- Binaries own deployment decisions: paths, startup modes, logging destinations, and dependency construction.
- `ConversationContext` is the only authority for committed conversation history. A message is committed only after append succeeds.
- Workflow orchestrates and writes conversation messages; state stores runtime state; adapters translate providers; tools execute actions; TUI projects state and emits commands.
- Library code must not hard-code the application root or read deployment policy from global state.
- Work is complete only after `cargo fmt`, `cargo test`, and `cargo run --bin appleby -- --help` pass.

## Work Mode

1. Decide whether the function exists in `FEATURE.md`.
2. Open or create the current day's `docs/QA/YYYY-MM-DD.md`.
3. Record each concrete question as `YYYY-MM-DD-Q<n>`.
4. After the user answers or a decision is made, append the corresponding `YYYY-MM-DD-A<n>` with the answer, decision, and reason. Do not leave the decision only in chat.
5. Implement only the decided function and chosen path.
6. Delete unapproved features and replaced implementations.
7. Verify the declared feature behavior and information path with tests.
