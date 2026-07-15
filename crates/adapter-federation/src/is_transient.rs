//! Classify an authorization failure as transient (retry) or permanent (skip).

use federation::AuthError;

/// Whether an authorization failure is expected to resolve on its own — so the
/// consumer should stop and retry the event later — rather than being permanent,
/// where leaving the cursor stuck before it would stall the whole peer feed.
///
/// * **Transient:** the scope's owner has not claimed yet (`Unowned`, also raised
///   when a parent row is not yet replicated), the signer's key has not propagated
///   (`UnknownNode`), or the control plane blipped (`Registry`). These let a
///   just-created server replicate once its owner claims it, instead of dropping
///   the events.
/// * **Permanent:** the signer is no longer the owner (`NotOwner`) or is fenced at
///   a stale epoch (`StaleEpoch`) — superseded by a rehoming; the row places in no
///   scope (`ScopeMismatch`); or the signature is bad (`Fed`). None ever becomes
///   applicable, so the event is skipped and the cursor advances past it.
pub fn is_transient(err: &AuthError) -> bool {
    matches!(
        err,
        AuthError::Unowned | AuthError::UnknownNode | AuthError::Registry(_)
    )
}
