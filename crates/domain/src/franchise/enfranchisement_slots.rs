//! Layer 2 — the enfranchisement rate cap.

/// Layer 2 — how many *new* citizens a server may admit right now.
///
/// The citizen roll may grow by at most 10% per 30 days, with a floor of 5 so
/// tiny servers can still grow. Members who qualify beyond the cap queue by
/// qualification date; nobody is ever denied, only delayed. This is the layer
/// that most directly answers "don't let a flood take over": even 10,000
/// qualified newcomers cannot outnumber 100 established citizens in one move.
///
/// Returns the number of open admission slots given the current citizen count
/// and how many were admitted in the trailing 30-day window.
pub fn enfranchisement_slots(citizen_count: u64, admitted_last_30d: u64) -> u64 {
    const FLOOR: u64 = 5;
    // ceil(10% of citizen_count), i.e. (citizen_count * 10 + 99) / 100.
    let ten_percent = citizen_count.saturating_mul(10).saturating_add(99) / 100;
    let cap = ten_percent.max(FLOOR);
    cap.saturating_sub(admitted_last_30d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_cap_floor_applies_to_small_server() {
        // 3 citizens: 10% rounds to 1, but the floor of 5 governs.
        assert_eq!(enfranchisement_slots(3, 0), 5);
        assert_eq!(enfranchisement_slots(3, 2), 3);
    }

    #[test]
    fn rate_cap_ten_percent_governs_large_server() {
        assert_eq!(enfranchisement_slots(100, 0), 10);
        assert_eq!(enfranchisement_slots(200, 5), 15);
    }

    #[test]
    fn flood_cannot_outpace_the_cap() {
        // 100 established citizens, 10 already admitted this window: 0 more,
        // no matter how many newcomers qualify.
        assert_eq!(enfranchisement_slots(100, 10), 0);
        assert_eq!(enfranchisement_slots(100, 50), 0); // saturating, never panics
    }
}
