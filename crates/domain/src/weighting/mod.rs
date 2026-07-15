//! How a server values a citizen's vote. A server may vote to weight ballots (by
//! contribution, tenure, or explicit grant), but weighting only ever adjusts the
//! ballot of someone who is *already* a citizen by criteria — it is never a path
//! into the franchise.

pub mod max_vote_weight;
pub mod vote_weighting;
pub mod weighting_scope;
