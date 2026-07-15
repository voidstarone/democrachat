//! A command-line driving adapter over the `app` use-cases. It knows nothing
//! about storage; the composition root injects a wired [`Services`].
//!
//! The `--now` global option lets the demo move the clock forward to age
//! accounts and dwell time — the composition root owns the actual clock and
//! passes a setter, so this adapter stays decoupled from which clock is in use.

use app::{EnfranchiseOutcome, Services};
use clap::{Parser, Subcommand};
use domain::{build_message_tree, MessageNode, Timestamp, Unmet};

#[derive(Parser)]
#[command(name = "democrachat", about = "A self-governing, Discord-style chat platform")]
pub struct Cli {
    /// Evaluate the command as if "now" were this unix timestamp (seconds).
    /// Handy for demonstrating the time-based franchise rules.
    #[arg(long, global = true)]
    now: Option<i64>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Register a new platform account.
    Register { handle: String },
    /// Found a new server; the founder becomes citizen #1.
    Found { founder: String, name: String },
    /// Join a server as an ordinary member (no vote).
    Join { handle: String, server: String },
    /// Attempt to become a citizen — succeeds only if the criteria are met.
    Enfranchise { handle: String, server: String },
    /// Show how a member stands against the franchise criteria.
    Status { handle: String, server: String },
    /// Show a server: its phase, citizen count, and governance surface.
    Show { server: String },
    /// Dev helper: set a member's endorsed-contribution score (simulates
    /// citizens reacting positively to their messages).
    SetContribution { handle: String, server: String, amount: i64 },
    /// Create a channel (founder-only, Seed phase).
    CreateChannel {
        founder: String,
        server: String,
        name: String,
        #[arg(default_value = "")]
        topic: String,
    },
    /// List a server's channels.
    Channels { server: String },
    /// Post a message to a channel.
    Post { handle: String, server: String, channel: String, body: String },
    /// Reply to a message, threading under it.
    Reply { handle: String, message: u64, body: String },
    /// Edit one of your own messages.
    Edit { handle: String, message: u64, body: String },
    /// Delete one of your own messages.
    Delete { handle: String, message: u64 },
    /// React to a message (a citizen's reaction endorses the author).
    React { handle: String, message: u64, emoji: String },
    /// Remove your reaction from a message.
    Unreact { handle: String, message: u64, emoji: String },
    /// Print a channel's messages as a thread tree, with reaction tallies.
    Thread { server: String, channel: String },
}

/// Parse the process arguments and run one command against `services`.
/// `set_now` is invoked with the `--now` override when present.
pub fn run(services: &Services, set_now: impl Fn(Timestamp)) -> i32 {
    let cli = Cli::parse();
    if let Some(secs) = cli.now {
        set_now(Timestamp(secs));
    }
    match dispatch(services, cli.command) {
        Ok(msg) => {
            println!("{msg}");
            0
        }
        Err(msg) => {
            eprintln!("error: {msg}");
            1
        }
    }
}

fn dispatch(services: &Services, command: Command) -> Result<String, String> {
    match command {
        Command::Register { handle } => {
            let user = services.register_account(&handle).map_err(|e| e.to_string())?;
            Ok(format!("registered {} (id {})", user.handle, user.id))
        }
        Command::Found { founder, name } => {
            let server = services.found_server(&founder, &name).map_err(|e| e.to_string())?;
            Ok(format!(
                "founded g/{} ({}) — {} is citizen #1",
                server.slug, server.name, founder
            ))
        }
        Command::Join { handle, server } => {
            services.join_server(&handle, &server).map_err(|e| e.to_string())?;
            Ok(format!("{handle} joined g/{server} as a member (no vote yet)"))
        }
        Command::Enfranchise { handle, server } => {
            let outcome = services.try_enfranchise(&handle, &server).map_err(|e| e.to_string())?;
            Ok(match outcome {
                EnfranchiseOutcome::Admitted => {
                    format!("✓ {handle} is now a CITIZEN of g/{server} — earned by meeting the criteria")
                }
                EnfranchiseOutcome::NotEligible(unmet) => {
                    let mut s = format!("✗ {handle} is NOT eligible in g/{server}:");
                    for u in &unmet {
                        s.push_str(&format!("\n    - {}", describe_unmet(u)));
                    }
                    s
                }
                EnfranchiseOutcome::RateCapped { admitted_this_window } => format!(
                    "⏳ {handle} qualifies but g/{server}'s enfranchisement rate cap is full \
                     ({admitted_this_window} admitted in the last 30 days). Not denied — queued."
                ),
            })
        }
        Command::Status { handle, server } => {
            let e = services.eligibility(&handle, &server).map_err(|e| e.to_string())?;
            if e.is_eligible() {
                Ok(format!("{handle} meets every franchise criterion in g/{server} (run `enfranchise`)"))
            } else {
                let mut s = format!("{handle} does not yet qualify in g/{server}:");
                for u in &e.unmet {
                    s.push_str(&format!("\n    - {}", describe_unmet(u)));
                }
                Ok(s)
            }
        }
        Command::Show { server } => {
            let (g, phase, citizens) = services
                .server_snapshot(&server)
                .ok_or_else(|| format!("no such server: '{server}'"))?;
            let mut surface: Vec<String> =
                g.enabled_ballots.iter().map(|b| format!("{b:?}")).collect();
            surface.sort();
            Ok(format!(
                "g/{} ({})\n  phase: {phase:?} ({citizens} citizen(s))\n  franchise: \
                 account≥{}d, member≥{}d, contribution≥{}\n  votes on: {}",
                g.slug,
                g.name,
                g.criteria.min_account_age_days,
                g.criteria.min_membership_days,
                g.criteria.min_contribution,
                surface.join(", ")
            ))
        }
        Command::SetContribution { handle, server, amount } => {
            services
                .set_contribution(&handle, &server, amount)
                .map_err(|e| e.to_string())?;
            Ok(format!("set {handle}'s contribution in g/{server} to {amount}"))
        }
        Command::CreateChannel { founder, server, name, topic } => {
            let ch = services
                .create_channel(&founder, &server, &name, &topic)
                .map_err(|e| e.to_string())?;
            Ok(format!("created #{} in g/{server}", ch.name))
        }
        Command::Channels { server } => {
            let channels = services
                .list_channels(&server)
                .ok_or_else(|| format!("no such server: '{server}'"))?;
            if channels.is_empty() {
                return Ok(format!("g/{server} has no channels yet"));
            }
            let mut s = format!("g/{server} channels:");
            for c in channels {
                let topic = if c.topic.is_empty() { String::new() } else { format!(" — {}", c.topic) };
                s.push_str(&format!("\n  #{}{topic}", c.name));
            }
            Ok(s)
        }
        Command::Post { handle, server, channel, body } => {
            let m = services
                .post_message(&handle, &server, &channel, &body)
                .map_err(|e| e.to_string())?;
            Ok(format!("posted message {} to #{channel}", m.id))
        }
        Command::Reply { handle, message, body } => {
            let m = services.reply_message(&handle, message, &body).map_err(|e| e.to_string())?;
            Ok(format!("posted reply {} under message {message}", m.id))
        }
        Command::Edit { handle, message, body } => {
            services.edit_message(&handle, message, &body).map_err(|e| e.to_string())?;
            Ok(format!("edited message {message}"))
        }
        Command::Delete { handle, message } => {
            services.delete_message(&handle, message).map_err(|e| e.to_string())?;
            Ok(format!("deleted message {message}"))
        }
        Command::React { handle, message, emoji } => {
            services.react(&handle, message, &emoji).map_err(|e| e.to_string())?;
            Ok(format!("{handle} reacted {emoji} to message {message}"))
        }
        Command::Unreact { handle, message, emoji } => {
            services.unreact(&handle, message, &emoji).map_err(|e| e.to_string())?;
            Ok(format!("{handle} removed {emoji} from message {message}"))
        }
        Command::Thread { server, channel } => {
            let messages = services
                .channel_messages(&server, &channel)
                .map_err(|e| e.to_string())?;
            if messages.is_empty() {
                return Ok(format!("#{channel} is empty"));
            }
            let tree = build_message_tree(&messages);
            let mut out = format!("#{channel} in g/{server}:");
            for node in &tree {
                render_node(services, node, 0, &mut out);
            }
            Ok(out)
        }
    }
}

/// Recursively render a message and its replies, indented by depth.
fn render_node(services: &Services, node: &MessageNode, depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth + 1);
    let m = &node.message;
    let author = services
        .user_handle(m.author)
        .unwrap_or_else(|| format!("user{}", m.author));
    let body = if m.is_deleted { "[deleted]".to_string() } else { m.body.clone() };
    let edited = if m.edited_at.is_some() { " (edited)" } else { "" };
    let reactions = services.message_reactions(m.id.0);
    let react_str = if reactions.is_empty() {
        String::new()
    } else {
        let parts: Vec<String> = reactions.iter().map(|(e, n)| format!("{e}{n}")).collect();
        format!("   [{}]", parts.join(" "))
    };
    out.push_str(&format!("\n{indent}[{}] {author}: {body}{edited}{react_str}", m.id));
    for child in &node.replies {
        render_node(services, child, depth + 1, out);
    }
}

fn describe_unmet(u: &Unmet) -> String {
    match u {
        Unmet::AccountTooYoung { need_days, have_days } => {
            format!("account too young: need {need_days}d, have {have_days}d")
        }
        Unmet::MembershipTooShort { need_days, have_days } => {
            format!("membership too short: need {need_days}d, have {have_days}d")
        }
        Unmet::InsufficientContribution { need, have } => {
            format!("not enough endorsed contribution: need {need}, have {have}")
        }
        Unmet::Sanctioned => "under an active sanction".to_string(),
        Unmet::Barred => "permanently barred from the franchise".to_string(),
    }
}
