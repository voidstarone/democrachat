/// The maximum clock skew (seconds) between a command's `issued_at` and the owner's
/// clock. A command outside `[now - SKEW, now + SKEW]` is refused as stale/future,
/// which bounds how long a captured command could ever be replayed.
pub const MAX_COMMAND_SKEW_SECS: i64 = 120;
