//! Layers 1 & 2 of the defense: the earned franchise (who *qualifies* to become
//! a citizen) and the enfranchisement rate cap (how fast the electorate may
//! *grow*).

pub mod eligibility;
pub mod email_franchise_rule;
pub mod enfranchisement_slots;
pub mod evaluate_eligibility;
pub mod franchise_criteria;
pub mod franchise_grace;
pub mod unmet;
