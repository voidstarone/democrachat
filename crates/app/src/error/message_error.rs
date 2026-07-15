//! Errors from posting, editing, deleting, or addressing a message.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MessageError {
    #[error("no such user: '{0}'")]
    NoSuchUser(String),
    #[error("no such server: '{0}'")]
    NoSuchServer(String),
    #[error("no such channel: '#{0}'")]
    NoSuchChannel(String),
    #[error("no such message: {0}")]
    NoSuchMessage(u64),
    #[error("'{0}' is not a member of this server and cannot post")]
    NotAMember(String),
    #[error("'{0}' is banned from this server")]
    Sanctioned(String),
    /// The author is currently muted; they may only post in the appeals channel.
    #[error("'{0}' is muted and can only post in #appeals")]
    Muted(String),
    #[error("only the author may edit or delete their message")]
    NotTheAuthor,
    #[error("message body must not be empty")]
    EmptyBody,
    #[error("a message may carry at most {0} attachments")]
    TooManyAttachments(usize),
    #[error("cannot reply to a message in a different channel")]
    CrossChannelReply,
    /// A plaintext post was attempted in an encrypted channel — the client must
    /// seal the body under the channel key and use the sealed-post path.
    #[error("this channel is encrypted; post a sealed message")]
    ChannelEncrypted,
    /// A sealed post was attempted in a channel that is not encrypted.
    #[error("this channel is not encrypted")]
    NotEncrypted,
}
