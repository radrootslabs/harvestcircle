// SPDX-License-Identifier: GPL-3.0-only
// Public fixture recipes and API readback follow Radroots Lib revision
// 189c49b74b4bafc142b00b76b296477931139e72's food_availability_conformance.rs
// (MIT OR Apache-2.0). This tool never signs or receives private keys.
#![forbid(unsafe_code)]

use radroots_blossom::{BlobDescriptor, BlobUrl, MediaType, Sha256};
use radroots_event::envelope::EventEnvelope;
use radroots_event::food::availability::{
    FoodAvailabilityDetails, FoodAvailabilityDetailsParts, FoodAvailabilityError,
    FoodAvailabilityImage, FoodAvailabilityStatus, FoodContent, FoodCurrency, FoodIdentifier,
    FoodImageDimensions, FoodPrice, FoodPublishedAt, FoodQuantity, FoodText, FoodUnit,
};
use radroots_event::listing::classified::ClassifiedListingPartition;
use radroots_event::media::AuthoredImage;
use radroots_event::wire::Nip01EventWire;
use radroots_event_codec::admission::food_availability::{
    RadrootsFoodAvailabilityAdmissionOutcome, validate_food_availability_revision,
    verify_and_admit_food_availability_event,
};
use radroots_event_codec::decode::food_availability::{
    RadrootsFoodAvailabilityProjectionOutcome, RadrootsInboundFoodAvailabilityProjection,
    project_verified_food_availability_event,
};
use radroots_event_codec::encode::food_availability::{
    RadrootsFoodAvailabilityEncodeError, authored_food_availability_build_tags,
    authored_food_availability_to_wire_parts,
};
use radroots_event_codec::verify::{RadrootsSignatureVerifiedEvent, verify_nip01_event};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path};

const REVISION: &str = "189c49b74b4bafc142b00b76b296477931139e72";
const SOURCE: &str = include_str!("../../source_profile.v1.json");
const SOURCE_SHA256: &str = "dede1eb1f682ecd548e8cd3ccb251d298762e504e2588a73528e5f7a5b9eff21";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Details {
    content: Value,
    identifier: String,
    title: String,
    summary: String,
    published_at: u64,
    location: String,
    price: Price,
    quantity: Option<Quantity>,
    status: String,
    images: Vec<Image>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Price {
    amount: String,
    currency: String,
    unit: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Quantity {
    amount: String,
    unit: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Image {
    bytes_utf8: String,
    url: String,
    media_type: String,
    uploaded_at: u64,
    dimensions: String,
}

fn text(recipe: &Value) -> String {
    if let Some(value) = recipe.get("value").and_then(Value::as_str) {
        assert_eq!(recipe.as_object().unwrap().len(), 1);
        value.to_owned()
    } else {
        assert_eq!(recipe.as_object().unwrap().len(), 2);
        let character = recipe["repeat"].as_str().unwrap();
        assert_eq!(character.chars().count(), 1);
        let count = usize::try_from(recipe["count"].as_u64().unwrap()).unwrap();
        assert!(count <= 262_144, "fixture recipe exceeds tooling bound");
        character.repeat(count)
    }
}

fn details(input: &Value) -> FoodAvailabilityDetails {
    let input: Details = serde_json::from_value(input.clone()).unwrap();
    let images = input
        .images
        .iter()
        .map(|image| {
            let bytes = image.bytes_utf8.as_bytes();
            let media_type = MediaType::parse(&image.media_type).unwrap();
            let verified = BlobDescriptor::new(
                BlobUrl::parse(&image.url).unwrap(),
                Sha256::digest(bytes),
                u64::try_from(bytes.len()).unwrap(),
                media_type.clone(),
                image.uploaded_at,
            )
            .unwrap()
            .approve_reference()
            .unwrap()
            .verify_bytes(bytes, &media_type)
            .unwrap();
            FoodAvailabilityImage::new(
                AuthoredImage::try_from_verified_descriptor(verified).unwrap(),
                FoodImageDimensions::parse(&image.dimensions).unwrap(),
            )
        })
        .collect();
    FoodAvailabilityDetails::new(FoodAvailabilityDetailsParts {
        content: FoodContent::new(text(&input.content)).unwrap(),
        identifier: FoodIdentifier::parse(&input.identifier).unwrap(),
        title: FoodText::new(input.title).unwrap(),
        summary: FoodText::new(input.summary).unwrap(),
        published_at: FoodPublishedAt::new(input.published_at).unwrap(),
        location: FoodText::new(input.location).unwrap(),
        price: FoodPrice::new(
            input.price.amount,
            FoodCurrency::parse(&input.price.currency).unwrap(),
            FoodUnit::parse(&input.price.unit).unwrap(),
        )
        .unwrap(),
        quantity: input.quantity.map(|quantity| {
            FoodQuantity::new(quantity.amount, FoodUnit::parse(&quantity.unit).unwrap()).unwrap()
        }),
        status: FoodAvailabilityStatus::parse(&input.status).unwrap(),
        images,
    })
    .unwrap()
}

fn envelope(value: &Value) -> EventEnvelope {
    let raw = serde_json::to_string(value).unwrap();
    let wire = Nip01EventWire::parse_json(&raw).unwrap();
    wire.verify_id().unwrap();
    let event = wire.into_envelope().unwrap();
    // Check every signed field after the real wire parser, before admission.
    assert_eq!(event.id_hex(), value["id"].as_str().unwrap());
    assert_eq!(event.author().to_hex(), value["pubkey"].as_str().unwrap());
    assert_eq!(
        event.created_at_u64(),
        value["created_at"].as_u64().unwrap()
    );
    assert_eq!(u64::from(event.kind_u32()), value["kind"].as_u64().unwrap());
    assert_eq!(json!(event.tags_as_vec()), value["tags"]);
    assert_eq!(event.content(), value["content"].as_str().unwrap());
    event
}

fn verified(value: &Value) -> RadrootsSignatureVerifiedEvent {
    verify_nip01_event(envelope(value)).unwrap()
}

fn partition(value: ClassifiedListingPartition) -> &'static str {
    match value {
        ClassifiedListingPartition::FocusedFoodAvailability => "focused_food_availability",
        ClassifiedListingPartition::OperationalListing => "operational_listing",
        ClassifiedListingPartition::GenericNip99 => "generic_nip99",
        ClassifiedListingPartition::Ambiguous => "ambiguous",
    }
}

fn projection(value: &RadrootsInboundFoodAvailabilityProjection) -> Value {
    let images: Vec<_> = value
        .images()
        .iter()
        .map(|image| {
            let dimensions = image
                .dimensions()
                .map(|size| json!({"width":size.width(),"height":size.height()}));
            json!({
                "raw_tag":image.raw_tag(),"url":image.url(),"dimensions":dimensions,
                "diagnostics":image.diagnostics().iter().map(|d|d.code()).collect::<Vec<_>>(),
                "qualifies":image.qualifies()
            })
        })
        .collect();
    json!({
        "content":value.content().as_str(),"identifier":value.identifier().as_str(),
        "title":value.title().as_str(),"summary":value.summary().as_str(),
        "published_at":value.published_at().as_u64(),"location":value.location().as_str(),
        "price":{"amount":value.price().amount(),"currency":value.price().currency().as_str(),"unit":value.price().unit().as_str()},
        "quantity":value.quantity().map(|q|json!({"amount":q.amount(),"unit":q.unit().as_str()})),
        "status":value.status().as_str(),"images":images,
        "diagnostics":value.diagnostics().iter().map(|d|d.code()).collect::<Vec<_>>()
    })
}

fn authored_error(error: &RadrootsFoodAvailabilityEncodeError) -> Value {
    match error {
        RadrootsFoodAvailabilityEncodeError::Domain(FoodAvailabilityError::PublishedAtFuture {
            published_at,
            created_at,
        }) => {
            json!({"code":error.code(),"message":error.to_string(),"published_at":published_at,"created_at":created_at})
        }
        RadrootsFoodAvailabilityEncodeError::EventWireTooLarge { max, actual } => {
            json!({"code":error.code(),"message":error.to_string(),"max":max,"actual":actual})
        }
        _ => json!({"code":error.code(),"message":error.to_string()}),
    }
}

fn actual(vector: &Value) -> Value {
    let kind = vector["kind"].as_str().unwrap();
    let input = &vector["input"];
    match kind {
        "food_availability.build_authored_draft.valid"
        | "food_availability.build_authored_draft.invalid" => {
            let details = details(&input["details"]);
            let created_at = input["created_at"].as_u64().unwrap();
            match authored_food_availability_to_wire_parts(&details, created_at) {
                Ok(wire) => {
                    assert_eq!(
                        wire.tags,
                        authored_food_availability_build_tags(&details, created_at).unwrap()
                    );
                    if vector["expected"]["wire_parts"]
                        .get("content_length")
                        .is_some()
                    {
                        json!({"wire_parts":{"kind":wire.kind,"content_length":wire.content.len(),"tags":wire.tags}})
                    } else {
                        json!({"wire_parts":{"kind":wire.kind,"content":wire.content,"tags":wire.tags}})
                    }
                }
                Err(error) => json!({"error":authored_error(&error)}),
            }
        }
        "food_availability.project_verified_event.valid"
        | "food_availability.project_verified_event.invalid" => {
            let event = verified(&input["event"]);
            match project_verified_food_availability_event(&event) {
                Ok(RadrootsFoodAvailabilityProjectionOutcome::Focused(value))
                    if vector["expected"].get("projection").is_some() =>
                {
                    json!({"outcome":"focused","event_id":event.event().id_hex(),"projection":projection(&value)})
                }
                Ok(RadrootsFoodAvailabilityProjectionOutcome::Focused(value)) => {
                    json!({"outcome":"focused","event_id":event.event().id_hex(),"image_count":value.images().len(),"diagnostics":value.diagnostics().iter().map(|d|d.code()).collect::<Vec<_>>(),"first_raw_tag":value.images().first().unwrap().raw_tag(),"last_raw_tag":value.images().last().unwrap().raw_tag()})
                }
                Ok(RadrootsFoodAvailabilityProjectionOutcome::Excluded(value)) => {
                    json!({"outcome":"excluded","event_id":event.event().id_hex(),"partition":partition(value)})
                }
                Ok(_) => panic!("unqualified projection outcome"),
                Err(error) => json!({"error":{"code":error.code(),"message":error.to_string()}}),
            }
        }
        "food_availability.verify_and_admit_event.valid"
        | "food_availability.verify_and_admit_event.invalid" => {
            match verify_and_admit_food_availability_event(envelope(&input["event"])) {
                Ok(RadrootsFoodAvailabilityAdmissionOutcome::Admitted(event)) => {
                    json!({"outcome":"admitted","event_id":event.event().id_hex(),"projection":projection(event.projection())})
                }
                Ok(RadrootsFoodAvailabilityAdmissionOutcome::Excluded(event)) => {
                    json!({"outcome":"excluded","event_id":event.event().id_hex(),"partition":partition(event.partition())})
                }
                Ok(_) => panic!("unqualified admission outcome"),
                Err(error) => json!({"error":{"code":error.code(),"message":error.to_string()}}),
            }
        }
        "food_availability.validate_revision.valid"
        | "food_availability.validate_revision.invalid" => {
            let previous = verified(&input["previous"]);
            let current = verified(&input["current"]);
            match validate_food_availability_revision(&previous, &current) {
                Ok(()) => json!({"result":"accepted","current_event_id":current.event().id_hex()}),
                Err(error) => json!({"error":{"code":error.code(),"message":error.to_string()}}),
            }
        }
        _ => panic!("unqualified vector kind {kind}"),
    }
}

fn reject_private_fields(value: &Value) {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                let key = key.to_ascii_lowercase();
                assert!(
                    !key.contains("secret")
                        && !key.contains("private_key")
                        && !key.contains("privkey")
                );
                reject_private_fields(value);
            }
        }
        Value::Array(values) => values.iter().for_each(reject_private_fields),
        _ => (),
    }
}

fn generate() -> Value {
    assert_eq!(Sha256::digest(SOURCE.as_bytes()).to_hex(), SOURCE_SHA256);
    let source: Value = serde_json::from_str(SOURCE).unwrap();
    reject_private_fields(&source);
    let vectors = source["vectors"].as_array().unwrap();
    assert_eq!(vectors.len(), 40);
    let mut ids = BTreeSet::new();
    let rows: Vec<_> = vectors.iter().map(|vector| {
        let id = vector["id"].as_str().unwrap();
        assert!(ids.insert(id));
        let result = actual(vector);
        assert_eq!(result, vector["expected"], "public Rust oracle drift: {id}");
        let mut signed_wires = serde_json::Map::new();
        for field in ["event", "previous", "current"] {
            if let Some(event) = vector["input"].get(field) {
                let raw = serde_json::to_string(event).unwrap();
                let wire = Nip01EventWire::parse_json(&raw).unwrap();
                wire.verify_id().unwrap();
                // Preserve exact parsed signature bytes even for the invalid-signature control.
                assert_eq!(wire.sig, event["sig"].as_str().unwrap());
                signed_wires.insert(field.to_owned(), Value::String(raw));
            }
        }
            let unsigned_wire_parts = if vector["kind"].as_str().unwrap().starts_with("food_availability.build_authored_draft.") {
                let details = details(&vector["input"]["details"]);
                authored_food_availability_to_wire_parts(&details, vector["input"]["created_at"].as_u64().unwrap())
                    .ok().map(|wire| json!({"kind":wire.kind,"content":wire.content,"tags":wire.tags}))
            } else {
                None
            };
            json!({"id":id,"kind":vector["kind"],"input":vector["input"],"expected":result,"signed_wires":signed_wires,"unsigned_wire_parts":unsigned_wire_parts})
    }).collect();
    json!({
        "schema_version":1,"repository":"https://github.com/radrootslabs/lib",
        "revision":REVISION,"source_path":"contracts/conformance/vectors/food_availability/profile.v1.json",
        "source_sha256":SOURCE_SHA256,"contract_version":source["contract_version"],
        "suite":source["suite"],"key_policy":"keyless verification of public signed fixtures; unsigned authoring only",
        "vectors":rows
    })
}

fn encoded() -> String {
    serde_json::to_string_pretty(&generate()).unwrap() + "\n"
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [mode] if mode == "--emit" => print!("{}", encoded()),
        [mode] if mode == "--check" => {
            let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../corpus.v1.json");
            let expected = fs::read_to_string(path).unwrap();
            assert!(expected.len() <= 1024 * 1024);
            assert_eq!(encoded(), expected, "checked corpus is stale");
            println!("PASS40 pinned public Rust vectors and exact generated bytes; no signing");
        }
        _ => panic!("expected exactly --emit or --check"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_corpus_matches_actual_pinned_rust_readback() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../corpus.v1.json");
        assert_eq!(encoded(), fs::read_to_string(path).unwrap());
        assert_eq!(
            encoded(),
            encoded(),
            "repeated generation must be byte-identical"
        );
    }
}
