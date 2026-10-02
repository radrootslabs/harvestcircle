//! Public raw lifecycle recipes; identity and signature bytes are produced in isolation.

pub const IDENTIFIER: &str = "hcav-lifecycle-v1";
pub const OTHER_IDENTIFIER: &str = "hcav-lifecycle-distinct";
// Synthetic protocol control only; this does not enable another product kind.
pub const OTHER_ADDRESSABLE_KIND: u16 = 30023;
pub const CUTOFF: u64 = 1_800_000_100;
pub const ALL_THREE_PERMUTATIONS: [[usize; 3]; 6] = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
];

pub enum Profile {
    Focused(&'static str),
    Generic,
}

pub struct Version {
    pub name: &'static str,
    pub created_at: u64,
    pub identifier: &'static str,
    pub profile: Profile,
    pub content: &'static str,
    pub other_author: bool,
}

pub const VERSIONS: [Version; 11] = [
    Version {
        name: "old_active",
        created_at: 1_800_000_100,
        identifier: IDENTIFIER,
        profile: Profile::Focused("active"),
        content: "Public lifecycle conformance.",
        other_author: false,
    },
    Version {
        name: "newer_sold",
        created_at: 1_800_000_200,
        identifier: IDENTIFIER,
        profile: Profile::Focused("sold"),
        content: "Public lifecycle conformance.",
        other_author: false,
    },
    Version {
        name: "tie_left",
        created_at: 1_800_000_300,
        identifier: IDENTIFIER,
        profile: Profile::Focused("active"),
        content: "Public equal-time left version.",
        other_author: false,
    },
    Version {
        name: "tie_right",
        created_at: 1_800_000_300,
        identifier: IDENTIFIER,
        profile: Profile::Focused("sold"),
        content: "Public equal-time right version.",
        other_author: false,
    },
    Version {
        name: "generic_head",
        created_at: 1_800_000_200,
        identifier: IDENTIFIER,
        profile: Profile::Generic,
        content: "Public unsupported generic version.",
        other_author: false,
    },
    Version {
        name: "malformed_head",
        created_at: 1_800_000_200,
        identifier: IDENTIFIER,
        profile: Profile::Focused("unsupported"),
        content: "Public unsupported focused version.",
        other_author: false,
    },
    Version {
        name: "other_identifier",
        created_at: 1_800_000_100,
        identifier: OTHER_IDENTIFIER,
        profile: Profile::Focused("active"),
        content: "Public lifecycle conformance.",
        other_author: false,
    },
    Version {
        name: "other_author",
        created_at: 1_800_000_100,
        identifier: IDENTIFIER,
        profile: Profile::Focused("active"),
        content: "Public lifecycle conformance.",
        other_author: true,
    },
    Version {
        name: "before_cutoff",
        created_at: 1_800_000_099,
        identifier: IDENTIFIER,
        profile: Profile::Focused("active"),
        content: "Public version before cutoff.",
        other_author: false,
    },
    Version {
        name: "at_cutoff",
        created_at: 1_800_000_100,
        identifier: IDENTIFIER,
        profile: Profile::Focused("active"),
        content: "Public version at cutoff.",
        other_author: false,
    },
    Version {
        name: "after_cutoff",
        created_at: 1_800_000_101,
        identifier: IDENTIFIER,
        profile: Profile::Focused("active"),
        content: "Public version after cutoff.",
        other_author: false,
    },
];

pub enum Target {
    Event(&'static str),
    Address,
    EventAndAddress(&'static str),
}

pub struct Deletion {
    pub name: &'static str,
    pub created_at: u64,
    pub target: Target,
    pub other_author: bool,
}

pub const DELETIONS: [Deletion; 6] = [
    Deletion {
        name: "exact_newer",
        created_at: 1_800_000_000,
        target: Target::Event("newer_sold"),
        other_author: false,
    },
    Deletion {
        name: "address_cutoff",
        created_at: 1_800_000_100,
        target: Target::Address,
        other_author: false,
    },
    Deletion {
        name: "forged_exact",
        created_at: 1_800_000_300,
        target: Target::Event("newer_sold"),
        other_author: true,
    },
    Deletion {
        name: "forged_address",
        created_at: 1_800_000_300,
        target: Target::Address,
        other_author: true,
    },
    Deletion {
        name: "combined_cutoff",
        created_at: 1_800_000_100,
        target: Target::EventAndAddress("at_cutoff"),
        other_author: false,
    },
    Deletion {
        name: "unrelated_exact",
        created_at: 1_800_000_300,
        target: Target::Event("tie_left"),
        other_author: false,
    },
];

pub fn listing_tags(version: &Version) -> Vec<Vec<String>> {
    let mut tags = vec![
        vec!["d".into(), version.identifier.into()],
        vec!["title".into(), "Public lifecycle fixture".into()],
        vec!["summary".into(), "Public lifecycle conformance".into()],
        vec!["published_at".into(), "1800000000".into()],
        vec!["location".into(), "Protocol test location".into()],
        vec!["price".into(), "3.5".into(), "CAD".into()],
    ];
    match version.profile {
        Profile::Focused(status) => {
            tags.push(vec!["radroots:price_unit".into(), "lb".into()]);
            tags.push(vec!["status".into(), status.into()]);
        }
        Profile::Generic => tags.push(vec!["status".into(), "active".into()]),
    }
    tags
}
