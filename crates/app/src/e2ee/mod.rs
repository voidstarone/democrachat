//! End-to-end encryption primitives for the server-blind private surface (M7).
//!
//! The server only ever stores **ciphertext** for DMs, message bodies, and the
//! social graph; keys never leave the client in the clear (`docs/federation.md`
//! §5a/§5c). These are the primitives that make that possible:
//!
//! * **Device key** — an X25519 keypair ([`IdentitySecret`] / [`PublicIdentity`]).
//!   Others seal to the public half; only the user holds the secret.
//! * **Sealed box** — [`seal_to`] / [`open_sealed`]: encrypt to a public key with a
//!   fresh ephemeral key per message, so the server (holding the result) is blind.
//! * **Password-wrapped secret** — [`wrap_secret`] / [`unwrap_secret`]: the device
//!   secret encrypted under an Argon2id KEK from the user's password, the only form
//!   the server holds ([`WrappedSecret`]). Re-entering the password on a new device
//!   recovers the identity; losing it loses the E2EE history (the §5c trade-off).
//!
//! In the real system the seal/open/wrap/unwrap steps run **client-side**; they
//! live in `app` so the Rust CLI client and these tests exercise the full flow, and
//! so the wire/blob formats are defined in one place a JS client mirrors.

pub mod channel_key;
pub mod derive_kek;
pub mod derive_key;
pub mod e2ee_error;
pub mod identity_secret;
pub mod open;
pub mod open_message;
pub mod public_identity;
pub mod seal;
pub mod seal_message;
pub mod unwrap;
pub mod wrap;
pub mod wrapped_secret;

#[cfg(test)]
mod tests {
    use super::channel_key::ChannelKey;
    use super::identity_secret::IdentitySecret;
    use super::open::open_sealed;
    use super::open_message::open_channel_message;
    use super::public_identity::PublicIdentity;
    use super::seal::seal_to;
    use super::seal_message::seal_channel_message;
    use super::unwrap::unwrap_secret;
    use super::wrap::wrap_secret;

    #[test]
    fn a_message_sealed_to_a_key_opens_only_with_its_secret() {
        let recipient = IdentitySecret::generate();
        let sealed = seal_to(&recipient.public(), b"the vote is nay");
        assert_eq!(open_sealed(&recipient, &sealed).unwrap(), b"the vote is nay");

        // A different user cannot open it.
        let interloper = IdentitySecret::generate();
        assert!(open_sealed(&interloper, &sealed).is_err());
    }

    #[test]
    fn the_public_key_round_trips_through_hex() {
        let secret = IdentitySecret::generate();
        let pk = secret.public();
        let restored = PublicIdentity::from_hex(&pk.to_hex()).unwrap();
        // A sender who only has the hex public key can still seal to the user.
        let sealed = seal_to(&restored, b"hi");
        assert_eq!(open_sealed(&secret, &sealed).unwrap(), b"hi");
    }

    #[test]
    fn sealing_the_same_plaintext_twice_differs() {
        let recipient = IdentitySecret::generate();
        let a = seal_to(&recipient.public(), b"same");
        let b = seal_to(&recipient.public(), b"same");
        assert_ne!(a, b, "a fresh ephemeral key per message hides repetition");
    }

    #[test]
    fn tampering_with_a_sealed_message_is_detected() {
        let recipient = IdentitySecret::generate();
        let mut sealed = seal_to(&recipient.public(), b"aye");
        let last = sealed.len() - 1;
        sealed[last] ^= 0x01;
        assert!(open_sealed(&recipient, &sealed).is_err());
    }

    #[test]
    fn a_device_secret_round_trips_through_a_password_wrap() {
        let secret = IdentitySecret::generate();
        let pk = secret.public();
        let wrapped = wrap_secret("correct horse battery staple", &secret).unwrap();

        // Unwrapping with the password recovers a secret that matches the same
        // public key — i.e. the identity is truly recovered.
        let recovered = unwrap_secret("correct horse battery staple", &wrapped).unwrap();
        assert_eq!(recovered.public(), pk);

        // A message sealed to the public key opens with the recovered secret.
        let sealed = seal_to(&pk, b"recovered");
        assert_eq!(open_sealed(&recovered, &sealed).unwrap(), b"recovered");
    }

    #[test]
    fn the_wrong_password_cannot_unwrap_the_secret() {
        let secret = IdentitySecret::generate();
        let wrapped = wrap_secret("right password here!!", &secret).unwrap();
        assert!(unwrap_secret("wrong password here!!", &wrapped).is_err());
    }

    #[test]
    fn a_channel_message_round_trips_under_its_key() {
        let key = ChannelKey::generate();
        let sealed = seal_channel_message(&key, b"the meeting is at noon");
        assert_eq!(open_channel_message(&key, &sealed).unwrap(), b"the meeting is at noon");

        // A different channel key cannot open it.
        let other = ChannelKey::generate();
        assert!(open_channel_message(&other, &sealed).is_err());
    }

    #[test]
    fn a_channel_key_round_trips_through_a_grant() {
        // A grant is the channel key sealed to a member's device key.
        let key = ChannelKey::generate();
        let member = IdentitySecret::generate();
        let grant = seal_to(&member.public(), &key.to_bytes());

        let recovered_bytes: [u8; 32] =
            open_sealed(&member, &grant).unwrap().try_into().unwrap();
        let recovered = ChannelKey::from_bytes(recovered_bytes);

        // The recovered key opens a message sealed under the original.
        let sealed = seal_channel_message(&key, b"quorum reached");
        assert_eq!(open_channel_message(&recovered, &sealed).unwrap(), b"quorum reached");
    }

    #[test]
    fn tampering_with_a_channel_message_is_detected() {
        let key = ChannelKey::generate();
        let mut sealed = seal_channel_message(&key, b"aye");
        let last = sealed.len() - 1;
        sealed[last] ^= 0x01;
        assert!(open_channel_message(&key, &sealed).is_err());
    }

    #[test]
    fn the_wrapped_blob_does_not_expose_the_secret() {
        // What the server stores is opaque: the raw secret bytes never appear in it.
        // `to_bytes` is crate-internal, so this check is only possible in-crate.
        let secret = IdentitySecret::generate();
        let wrapped = wrap_secret("a decent passphrase!", &secret).unwrap();
        let raw_hex = hex::encode(secret.to_bytes());
        let blob = format!("{}{}{}", wrapped.salt, wrapped.nonce, wrapped.ciphertext);
        assert!(!blob.contains(&raw_hex), "the plaintext secret must not leak into the blob");
    }
}
