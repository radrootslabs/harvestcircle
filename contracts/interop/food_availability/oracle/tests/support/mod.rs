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

pub(super) fn actual(vector: &Value) -> Value {
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WebTemplate {
    id: String,
    created_at: u64,
    wire_parts: WebParts,
    summary: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WebParts {
    kind: u32,
    content: String,
    tags: Vec<Vec<String>>,
}

pub(super) fn consume_template(template: &WebTemplate) -> Result<Value, String> {
    if template.id.is_empty()
        || template.id.len() > 128
        || template.created_at > 9_007_199_254_740_991
        || template.wire_parts.kind != 30402
        || template.wire_parts.content.len() > 16 * 1024
        || template.wire_parts.tags.len() > 9
    {
        return Err("unsupported bounded website template".into());
    }
    let value = |name: &str| -> Result<&str, String> {
        let tags = template
            .wire_parts
            .tags
            .iter()
            .filter(|tag| tag.first().is_some_and(|key| key == name))
            .collect::<Vec<_>>();
        match tags.as_slice() {
            [tag] if tag.len() == 2 => Ok(tag[1].as_str()),
            _ => Err("invalid website singleton tag".into()),
        }
    };
    let price_tags = template
        .wire_parts
        .tags
        .iter()
        .filter(|tag| tag.first().is_some_and(|key| key == "price"))
        .collect::<Vec<_>>();
    let price = match price_tags.as_slice() {
        [tag] if tag.len() == 3 => *tag,
        _ => return Err("invalid website price tag".into()),
    };
    let quantity_tags = template
        .wire_parts
        .tags
        .iter()
        .filter(|tag| tag.first().is_some_and(|key| key == "radroots:quantity"))
        .collect::<Vec<_>>();
    let quantity = match quantity_tags.as_slice() {
        [] => None,
        [tag] if tag.len() == 3 => Some(
            FoodQuantity::new(
                &tag[1],
                FoodUnit::parse(&tag[2]).map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?,
        ),
        _ => return Err("invalid website quantity tag".into()),
    };
    if value("summary")? != template.summary {
        return Err("website review summary differs from public template".into());
    }
    // Actual pinned public constructors and codec consume the website fields.
    // No synthetic signature, signer, native runtime or private key is involved.
    let details = FoodAvailabilityDetails::new(FoodAvailabilityDetailsParts {
        content: FoodContent::new(&template.wire_parts.content)
            .map_err(|error| error.to_string())?,
        identifier: FoodIdentifier::parse(value("d")?).map_err(|error| error.to_string())?,
        title: FoodText::new(value("title")?).map_err(|error| error.to_string())?,
        summary: FoodText::new(value("summary")?).map_err(|error| error.to_string())?,
        published_at: FoodPublishedAt::parse(value("published_at")?)
            .map_err(|error| error.to_string())?,
        location: FoodText::new(value("location")?).map_err(|error| error.to_string())?,
        price: FoodPrice::new(
            &price[1],
            FoodCurrency::parse(&price[2]).map_err(|error| error.to_string())?,
            FoodUnit::parse(value("radroots:price_unit")?).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?,
        quantity,
        status: FoodAvailabilityStatus::parse(value("status")?)
            .map_err(|error| error.to_string())?,
        images: Vec::new(),
    })
    .map_err(|error| error.to_string())?;
    let actual = authored_food_availability_to_wire_parts(&details, template.created_at)
        .map_err(|error| error.to_string())?;
    if actual.kind != template.wire_parts.kind
        || actual.content != template.wire_parts.content
        || actual.tags != template.wire_parts.tags
    {
        return Err("website wire parts differ from actual pinned Rust codec".into());
    }
    Ok(
        json!({"id":template.id,"result":"accepted","wire_parts":{"kind":actual.kind,"content":actual.content,"tags":actual.tags}}),
    )
}

pub(super) fn boundary(row: &Value) -> Value {
    let value = row["value"].as_str().unwrap();
    let (identifier, timestamp) = match row["kind"].as_str().unwrap() {
        "timestamp" => match FoodPublishedAt::parse(value) {
            Ok(timestamp) => ("boundary-id", timestamp.as_u64()),
            Err(error) => return json!({"result":"rejected","code":error.code()}),
        },
        "identifier" => (value, 1),
        _ => panic!("unknown boundary kind"),
    };
    let recipe = json!({
        "content":{"value":"Fresh food available locally."},
        "identifier":identifier,"title":"Fresh food","summary":"Fresh food available locally.",
        "published_at":timestamp,"location":"Victoria",
        "price":{"amount":"1","currency":"CAD","unit":"g"},
        "quantity":null,"status":"active","images":[]
    });
    let wire = authored_food_availability_to_wire_parts(&details(&recipe), timestamp).unwrap();
    let tag = |name: &str| {
        wire.tags
            .iter()
            .find(|tag| tag.first().is_some_and(|key| key == name))
            .unwrap()[1]
            .clone()
    };
    if row["kind"] == "identifier" {
        json!({"result":"accepted","identifier":tag("d")})
    } else {
        json!({"result":"accepted","timestamp":timestamp.to_string(),"encoded_published_at":tag("published_at")})
    }
}
