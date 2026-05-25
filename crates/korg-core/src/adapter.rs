use crate::event::NormalizedEvent;

/// Plugin contract for wire-format translators.
///
/// Each Adapter handles one external agent protocol (Codex WebSocket frames,
/// Grok NDJSON, etc.) and translates it into korg's NormalizedEvent.
///
/// # In-process vs out-of-process adapters
///
/// This trait is for **in-process Rust adapters** — they are compiled into the korg
/// binary, receive compile-time type checking, and have zero serialization overhead.
///
/// **Out-of-process adapters** (the Python packages in `adapters/`) adhere to the
/// same conceptual contract (`source_agent_prefix`, normalize) but as HTTP clients
/// posting to `/api/agent-tool-call`. They get **runtime validation only** — no
/// trait, no compile-time guarantee. See Architecture/Overview.md §Adapter tiers.
///
/// # Object safety
///
/// The trait is object-safe: `Box<dyn Adapter>` works. All methods take `&self`
/// or `serde_json::Value` (type-erased). This mirrors Bevy's Plugin trait, which
/// is also object-safe despite Plugin impls being concrete typed at registration.
pub trait Adapter: Send + Sync + 'static {
    /// Human-readable identifier, e.g. "codex-ws" or "grok-heavy".
    fn name(&self) -> &str;

    /// SemVer version of this adapter implementation, e.g. "0.1.0".
    ///
    /// Stamped into event metadata on every normalize() call. Different adapter
    /// versions may normalize the same wire format differently (bug fixes, schema
    /// changes), so audit and replay need to know which version processed a given
    /// event. This is the field that makes that possible.
    fn version(&self) -> &str;

    /// Prefix matched against incoming event source_agent fields for routing.
    ///
    /// Convention: `"agent:<name>@"` — matches any version of a named agent.
    /// Example: `"agent:korgex@"` routes all korgex events to this adapter.
    fn source_agent_prefix(&self) -> &str;

    /// Fast-fail validation before normalization.
    ///
    /// Called before `normalize()`. Return `Err` to reject malformed input without
    /// paying the cost of a full normalization attempt. Should be cheap — check
    /// presence of required keys, field types, and enum values.
    ///
    /// # Contract
    ///
    /// **`validate` is a cheap necessary-but-not-sufficient check.**
    ///
    /// The invariant is: `validate(x).is_err()` implies `normalize(x).is_err()`.
    /// validate must never pass input that normalize would reject.
    ///
    /// The converse does **not** hold: `normalize` can fail on inputs that passed
    /// `validate`. For example, a field may be present and correctly-typed per
    /// validate, but contain a value that normalize cannot map to `NormalizedEvent`
    /// (e.g., an unrecognized tool name, an out-of-range duration). `validate` is
    /// the fast pre-flight; `normalize` is the authoritative check.
    ///
    /// Adapter authors must not assume that a `validate(x)` returning `Ok(())` means
    /// `normalize(x)` will succeed — only that if validate fails, normalize would
    /// too. Design validate to catch the cheapest, highest-signal failures.
    fn validate(&self, raw: &serde_json::Value) -> anyhow::Result<()>;

    /// Translate raw wire-format JSON into a NormalizedEvent.
    ///
    /// Adapters own their own deserialization. `raw` is type-erased at the trait
    /// boundary because input shapes vary (WebSocket frames vs NDJSON lines vs
    /// HTTP responses) and a common input type would just be serde_json::Value
    /// in disguise with extra indirection.
    ///
    /// The `adapter_version` field in the resulting event metadata is populated
    /// by the caller (korg-runtime) from `self.version()` — adapters do not set it.
    /// This ensures the version stamp reflects the loaded adapter binary, not a
    /// hardcoded string that could drift from the actual implementation version.
    fn normalize(&self, raw: serde_json::Value) -> anyhow::Result<NormalizedEvent>;
}
