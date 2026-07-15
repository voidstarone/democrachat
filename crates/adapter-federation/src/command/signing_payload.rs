/// The exact bytes a forwarded command is signed over: a domain-separated join of
/// every authenticated field, so tampering with any one — including the anti-replay
/// metadata (`issued_at`, `nonce`) — breaks the signature.
pub(crate) fn signing_payload(node: u16, issued_at: i64, nonce: &str, body: &str) -> String {
    format!("democrachat:cmd:v1\n{node}\n{issued_at}\n{nonce}\n{body}")
}
