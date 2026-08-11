use nostr::EventId;
use nostr::nips::nip19::{FromBech32, Nip19, ToBech32};

const MAX_REFERENCE_BYTES: usize = 2_048;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NostrReferenceKind {
    EventId,
    PublicKey,
    Profile,
    Note,
    NostrEvent,
    Address,
    PrivateKeyRejected,
    Invalid,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NostrReferenceParse {
    pub classification: NostrReferenceKind,
    pub canonical_reference: Option<String>,
}

#[must_use]
pub fn classify_reference(raw: &str) -> NostrReferenceParse {
    if raw.is_empty()
        || raw.len() > MAX_REFERENCE_BYTES
        || raw.chars().any(char::is_control)
        || raw.trim() != raw
    {
        return invalid();
    }
    let value = raw.strip_prefix("nostr:").unwrap_or(raw);
    if value.is_empty() || value.starts_with("nostr:") {
        return invalid();
    }
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return EventId::from_hex(value).map_or_else(
            |_| invalid(),
            |event_id| valid(NostrReferenceKind::EventId, event_id.to_hex()),
        );
    }
    match Nip19::from_bech32(value) {
        Ok(Nip19::Secret(_)) => NostrReferenceParse {
            classification: NostrReferenceKind::PrivateKeyRejected,
            canonical_reference: None,
        },
        Ok(reference @ Nip19::Pubkey(_)) => canonical(NostrReferenceKind::PublicKey, reference),
        Ok(reference @ Nip19::Profile(_)) => canonical(NostrReferenceKind::Profile, reference),
        Ok(reference @ Nip19::EventId(_)) => canonical(NostrReferenceKind::Note, reference),
        Ok(reference @ Nip19::Event(_)) => canonical(NostrReferenceKind::NostrEvent, reference),
        Ok(reference @ Nip19::Coordinate(_)) => canonical(NostrReferenceKind::Address, reference),
        Err(_) => invalid(),
    }
}

fn canonical(kind: NostrReferenceKind, reference: Nip19) -> NostrReferenceParse {
    reference
        .to_bech32()
        .map_or_else(|_| invalid(), |value| valid(kind, value))
}

fn valid(kind: NostrReferenceKind, canonical_reference: String) -> NostrReferenceParse {
    NostrReferenceParse {
        classification: kind,
        canonical_reference: Some(canonical_reference),
    }
}

fn invalid() -> NostrReferenceParse {
    NostrReferenceParse {
        classification: NostrReferenceKind::Invalid,
        canonical_reference: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{NostrReferenceKind, classify_reference};

    const NPUB: &str = "npub14f8usejl26twx0dhuxjh9cas7keav9vr0v8nvtwtrjqx3vycc76qqh9nsy";
    const NSEC: &str = "nsec1j4c6269y9w0q2er2xjw8sv2ehyrtfxq3jwgdlxj6qfn8z4gjsq5qfvfk99";
    const NOTE: &str = "note1m99r7nwc0wdrkzldrqan96gklg5usqspq7z9696j6unf0ljnpxjspqfw99";
    const NPROFILE: &str = "nprofile1qqsrhuxx8l9ex335q7he0f09aej04zpazpl0ne2cgukyawd24mayt8gppemhxue69uhhytnc9e3k7mf0qyt8wumn8ghj7er2vfshxtnnv9jxkc3wvdhk6tclr7lsh";
    const NEVENT: &str = "nevent1qqsdhet4232flykq3048jzc9msmaa3hnxuesxy3lnc33vd0wt9xwk6szyqewrqnkx4zsaweutf739s0cu7et29zrntqs5elw70vlm8zudr3y24sqsgy";
    const NADDR: &str = "naddr1qqxnzd3exgersv33xymnsve3qgs8suecw4luyht9ekff89x4uacneapk8r5dyk0gmn6uwwurf6u9rusrqsqqqa282m3gxt";

    #[test]
    fn classifies_canonical_public_references_and_exact_event_hex() {
        for (value, expected) in [
            (NPUB, NostrReferenceKind::PublicKey),
            (NPROFILE, NostrReferenceKind::Profile),
            (NOTE, NostrReferenceKind::Note),
            (NEVENT, NostrReferenceKind::NostrEvent),
            (NADDR, NostrReferenceKind::Address),
            (
                "d94a3f4dd87b9a3b0bed183b32e916fa29c8020107845d1752d72697fe5309a5",
                NostrReferenceKind::EventId,
            ),
        ] {
            assert_eq!(classify_reference(value).classification, expected);
            assert_eq!(
                classify_reference(&format!("nostr:{value}")).classification,
                expected
            );
            assert!(classify_reference(value).canonical_reference.is_some());
        }
    }

    #[test]
    fn separates_private_keys_and_rejects_noncanonical_or_unbounded_input() {
        assert_eq!(
            classify_reference(NSEC).classification,
            NostrReferenceKind::PrivateKeyRejected
        );
        assert_eq!(
            classify_reference(&format!("nostr:{NSEC}")).classification,
            NostrReferenceKind::PrivateKeyRejected
        );
        assert!(classify_reference(NSEC).canonical_reference.is_none());
        for invalid in [
            "",
            " note1m99r7nwc0wdrkzldrqan96gklg5usqspq7z9696j6unf0ljnpxjspqfw99",
            "nostr:nostr:bad",
            "note1qqqqqq",
            "00",
            "npub1bad\n",
        ] {
            assert_eq!(
                classify_reference(invalid).classification,
                NostrReferenceKind::Invalid
            );
        }
        assert_eq!(
            classify_reference(&"a".repeat(2_049)).classification,
            NostrReferenceKind::Invalid
        );
    }
}
