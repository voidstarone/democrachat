//! How a server sizes the jury that judges a report.

use serde::{Deserialize, Serialize};

use crate::ContentScale;

/// How a server sizes the jury that judges a report — a *governable* policy
/// ([`crate::ProposalKind::SetJurySizing`]).
///
/// Whatever the law, the result is clamped to a strict **minority** of the
/// electorate (never half or more), and a server too small to seat a minority
/// panel (fewer than 3 citizens) holds no jury. The platform default is
/// [`JurySizing::Sqrt`]: the panel is a tiny share of a large server and a larger
/// share of a small one, so big communities aren't dragged into every report.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum JurySizing {
    /// Sub-linear: `target = factor × ⌊√citizens⌋`, the factor in basis points
    /// (`10_000` = ×1.0).
    Sqrt {
        post_factor_bp: u32,
        comment_factor_bp: u32,
    },
    /// Linear: `target = proportion × citizens` — the same share at every size.
    Proportion { post_bp: u32, comment_bp: u32 },
    /// A fixed panel size, independent of server size.
    Fixed { post: u32, comment: u32 },
}

impl Default for JurySizing {
    fn default() -> Self {
        // ×1.0·√n for posts, ×0.5·√n for comments.
        JurySizing::Sqrt {
            post_factor_bp: 10_000,
            comment_factor_bp: 5_000,
        }
    }
}

impl JurySizing {
    /// The number of jurors to empanel for `scale` content given `citizens`
    /// enfranchised citizens, clamped to a strict minority of the electorate.
    /// Returns `0` when the server is too small to seat a minority panel.
    pub fn jury_size(&self, citizens: u64, scale: ContentScale) -> usize {
        // The largest panel strictly smaller than half the electorate. With
        // fewer than 3 citizens this is 0 — no jury can be both ≥1 and a minority.
        let cap = citizens.saturating_sub(1) / 2;
        if cap == 0 {
            return 0;
        }
        let target = match (*self, scale) {
            (JurySizing::Sqrt { post_factor_bp, .. }, ContentScale::Post) => {
                citizens.isqrt() * post_factor_bp as u64 / 10_000
            }
            (JurySizing::Sqrt { comment_factor_bp, .. }, ContentScale::Comment) => {
                citizens.isqrt() * comment_factor_bp as u64 / 10_000
            }
            (JurySizing::Proportion { post_bp, .. }, ContentScale::Post) => {
                citizens * post_bp as u64 / 10_000
            }
            (JurySizing::Proportion { comment_bp, .. }, ContentScale::Comment) => {
                citizens * comment_bp as u64 / 10_000
            }
            (JurySizing::Fixed { post, .. }, ContentScale::Post) => post as u64,
            (JurySizing::Fixed { comment, .. }, ContentScale::Comment) => comment as u64,
        };
        target.clamp(1, cap) as usize
    }

    /// The law's canonical wire name — the single home for the string tag, paired
    /// with [`factors`](Self::factors) and [`from_mode`](Self::from_mode) so the
    /// web↔domain string mapping lives in one place. A new law is named here, not
    /// at each serialize/parse call site.
    pub const fn mode_name(self) -> &'static str {
        match self {
            JurySizing::Sqrt { .. } => "Sqrt",
            JurySizing::Proportion { .. } => "Proportion",
            JurySizing::Fixed { .. } => "Fixed",
        }
    }

    /// The `(post, comment)` parameter pair, whatever the law — the two numbers the
    /// wire carries alongside [`mode_name`](Self::mode_name).
    pub const fn factors(self) -> (u32, u32) {
        match self {
            JurySizing::Sqrt { post_factor_bp, comment_factor_bp } => (post_factor_bp, comment_factor_bp),
            JurySizing::Proportion { post_bp, comment_bp } => (post_bp, comment_bp),
            JurySizing::Fixed { post, comment } => (post, comment),
        }
    }

    /// Build a law from its wire `mode` name and `(post, comment)` parameters.
    /// `None` for an unknown mode — the inverse of [`mode_name`](Self::mode_name).
    pub fn from_mode(mode: &str, post: u32, comment: u32) -> Option<Self> {
        match mode {
            "Sqrt" => Some(JurySizing::Sqrt { post_factor_bp: post, comment_factor_bp: comment }),
            "Proportion" => Some(JurySizing::Proportion { post_bp: post, comment_bp: comment }),
            "Fixed" => Some(JurySizing::Fixed { post, comment }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_panel_shrinks_as_a_share_as_the_server_grows() {
        let p = JurySizing::default();
        assert_eq!(p.jury_size(10_000, ContentScale::Post), 100);
        assert_eq!(p.jury_size(100, ContentScale::Post), 10);
        assert_eq!(p.jury_size(10, ContentScale::Post), 3);
        assert!(p.jury_size(10, ContentScale::Post) < 5);
    }

    #[test]
    fn comments_draw_a_smaller_panel_than_posts() {
        let p = JurySizing::default();
        assert_eq!(p.jury_size(100, ContentScale::Post), 10);
        assert_eq!(p.jury_size(100, ContentScale::Comment), 5);
    }

    #[test]
    fn the_panel_is_never_a_majority() {
        let p = JurySizing::default();
        for citizens in 0..200u64 {
            let n = p.jury_size(citizens, ContentScale::Post) as u64;
            assert!(2 * n < citizens || n == 0, "{n} of {citizens} is not a minority");
        }
    }

    #[test]
    fn tiny_servers_seat_no_jury() {
        let p = JurySizing::default();
        assert_eq!(p.jury_size(0, ContentScale::Post), 0);
        assert_eq!(p.jury_size(2, ContentScale::Post), 0);
        assert_eq!(p.jury_size(3, ContentScale::Post), 1);
    }

    #[test]
    fn proportion_and_fixed_laws_also_stay_a_minority() {
        let prop = JurySizing::Proportion {
            post_bp: 6_000,
            comment_bp: 6_000,
        };
        assert_eq!(prop.jury_size(10, ContentScale::Post), 4); // not 6

        let fixed = JurySizing::Fixed { post: 50, comment: 50 };
        assert_eq!(fixed.jury_size(20, ContentScale::Post), 9);
        assert_eq!(fixed.jury_size(1_000, ContentScale::Post), 50);
    }

    /// The wire mapping round-trips: every law's `(mode_name, factors)` rebuilds the
    /// same law via `from_mode`.
    #[test]
    fn the_wire_mapping_round_trips() {
        for law in [
            JurySizing::Sqrt { post_factor_bp: 10_000, comment_factor_bp: 5_000 },
            JurySizing::Proportion { post_bp: 6_000, comment_bp: 3_000 },
            JurySizing::Fixed { post: 12, comment: 7 },
        ] {
            let (post, comment) = law.factors();
            assert_eq!(JurySizing::from_mode(law.mode_name(), post, comment), Some(law));
        }
        assert_eq!(JurySizing::from_mode("Nope", 1, 1), None);
    }
}
