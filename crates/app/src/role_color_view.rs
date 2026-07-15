//! Read-model: one custom role a member holds, with its voted-on colour.

/// One custom role a member holds, with its current (plurality) colour and the
/// viewer's own colour vote — enough for the popover to show a swatch and offer a
/// re-vote control.
pub struct RoleColorView {
    pub id: u64,
    pub name: String,
    /// The winning colour among franchised citizens (`#rrggbb`), if any voted.
    pub color: Option<String>,
    /// The viewing citizen's own colour vote, if any.
    pub my_color: Option<String>,
}
