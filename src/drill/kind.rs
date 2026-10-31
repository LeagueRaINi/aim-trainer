//! The stable identity of each built-in drill. Everything else about a drill is described by its
//! [`DrillDef`], returned by [`DrillKind::def`].

use serde::{Deserialize, Serialize};

use super::{def::DrillDef, kinds};

/// Persisted: the variant names are stored in the settings and custom-content RON files, and
/// replays store the index in [`DrillKind::ALL`]. Never rename or reorder; only append.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DrillKind {
    #[default]
    Gridshot,
    Microflick,
    Tracking,
    HeadLevel,
    Peek,
    Reaction,
    Peripheral,
}

impl DrillKind {
    /// Order matters: replays store the index. Only ever append.
    pub const ALL: [DrillKind; 7] = [
        DrillKind::Gridshot,
        DrillKind::Microflick,
        DrillKind::Tracking,
        DrillKind::HeadLevel,
        DrillKind::Peek,
        DrillKind::Reaction,
        DrillKind::Peripheral,
    ];

    /// The drill's description. This is the only per-kind `match`.
    pub fn def(self) -> &'static DrillDef {
        match self {
            DrillKind::Gridshot => &kinds::gridshot::DEF,
            DrillKind::Microflick => &kinds::microflick::DEF,
            DrillKind::Tracking => &kinds::tracking::DEF,
            DrillKind::HeadLevel => &kinds::head_level::DEF,
            DrillKind::Peek => &kinds::peek::DEF,
            DrillKind::Reaction => &kinds::reaction::DEF,
            DrillKind::Peripheral => &kinds::peripheral::DEF,
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.name() == name)
    }

    /// Display name; also the history CSV identifier (see [`DrillDef::name`]).
    pub fn name(self) -> &'static str {
        self.def().name
    }

    pub fn blurb(self) -> &'static str {
        self.def().blurb
    }

    /// The kinds the save-format fixtures (`fixtures/`) were written with: every kind that
    /// existed then. Kinds appended later don't change them.
    #[cfg(test)]
    pub fn fixture_kinds() -> &'static [DrillKind] {
        &Self::ALL[..7]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn def_table_is_consistent() {
        for (i, kind) in DrillKind::ALL.into_iter().enumerate() {
            // Names are history ids: unique, and found again by name.
            assert!(DrillKind::ALL[..i].iter().all(|other| other.name() != kind.name()), "{kind:?}");
            assert_eq!(DrillKind::from_name(kind.name()), Some(kind));
            // The defaults are already inside every limit.
            let defaults = crate::drill::DrillConfig::defaults(kind);
            let mut sanitized = defaults.clone();
            sanitized.sanitize();
            assert_eq!(sanitized, defaults, "{kind:?}");
        }
    }
}
