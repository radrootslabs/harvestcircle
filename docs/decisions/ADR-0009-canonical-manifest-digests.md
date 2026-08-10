# ADR-0009: Manifest digests use canonical semantic serialization

Status: Accepted

Raw checkout bytes are not portable across line-ending policies.

Product and provenance digests are computed from parsed and canonicalized
content.

LF policy remains a defense in depth, not the digest authority.
