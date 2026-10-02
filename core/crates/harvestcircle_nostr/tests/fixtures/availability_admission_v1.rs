//! Bounded public incoming tag recipes, independent of strict writer constructors.

use radroots_event::food::availability::FoodAvailabilityError;
use radroots_event_codec::admission::food_availability::RadrootsFoodAvailabilityAdmissionError;
use radroots_event_codec::decode::food_availability::RadrootsFoodAvailabilityProjectionError;

pub const CREATED_AT: u64 = 1_800_000_100;
pub const CONTENT: &str = "Public incoming availability conformance.";
pub const FOCUSED_MARKERS: [&str; 2] = ["radroots:price_unit", "radroots:quantity"];
pub const OPERATIONAL_MARKERS: [&str; 3] =
    ["radroots:primary_bin", "radroots:bin", "radroots:price"];

pub const BASE_TAGS: &[&[&str]] = &[
    &["d", "hcav-incoming-v1"],
    &["title", "Incoming availability conformance"],
    &["summary", "Public incoming protocol fixture"],
    &["published_at", "1800000000"],
    &["location", "Protocol test location"],
    &["price", "0003.5000", "cad"],
    &["radroots:price_unit", "lb"],
    &["status", "active"],
];

pub struct Recipe {
    pub content: String,
    pub tags: Vec<Vec<String>>,
}

pub enum Edit {
    Remove(&'static str),
    Replace(&'static str, &'static [&'static str]),
    Append(&'static [&'static str]),
    Content(&'static str),
}

pub struct MalformedCase {
    pub name: &'static str,
    pub edits: &'static [Edit],
    pub expected: RadrootsFoodAvailabilityAdmissionError,
    pub code: &'static str,
}

pub const MALFORMED_CASES: [MalformedCase; 18] = [
    MalformedCase {
        name: "missing_title",
        edits: &[Edit::Remove("title")],
        expected: projection(RadrootsFoodAvailabilityProjectionError::TagInvalid),
        code: "food_tag_invalid",
    },
    MalformedCase {
        name: "duplicate_title",
        edits: &[Edit::Append(&["title", "Second title"])],
        expected: projection(RadrootsFoodAvailabilityProjectionError::TagInvalid),
        code: "food_tag_invalid",
    },
    MalformedCase {
        name: "title_extra_element",
        edits: &[Edit::Replace("title", &["title", "Title", "extra"])],
        expected: projection(RadrootsFoodAvailabilityProjectionError::TagInvalid),
        code: "food_tag_invalid",
    },
    MalformedCase {
        name: "untrimmed_title",
        edits: &[Edit::Replace("title", &["title", " Untrimmed title"])],
        expected: domain(FoodAvailabilityError::TextInvalid),
        code: "food_text_invalid",
    },
    MalformedCase {
        name: "identifier_whitespace",
        edits: &[Edit::Replace("d", &["d", "bad identifier"])],
        expected: domain(FoodAvailabilityError::IdentifierInvalid),
        code: "food_identifier_invalid",
    },
    MalformedCase {
        name: "empty_content",
        edits: &[Edit::Content("")],
        expected: domain(FoodAvailabilityError::ContentMissing),
        code: "food_content_missing",
    },
    MalformedCase {
        name: "future_publication",
        edits: &[Edit::Replace(
            "published_at",
            &["published_at", "1800000101"],
        )],
        expected: domain(FoodAvailabilityError::PublishedAtFuture {
            published_at: 1_800_000_101,
            created_at: 1_800_000_100,
        }),
        code: "food_published_at_future",
    },
    MalformedCase {
        name: "price_exponent",
        edits: &[Edit::Replace("price", &["price", "1e3", "CAD"])],
        expected: domain(FoodAvailabilityError::PriceInvalid),
        code: "price_invalid",
    },
    MalformedCase {
        name: "price_frequency",
        edits: &[Edit::Replace("price", &["price", "1", "CAD", "day"])],
        expected: projection(RadrootsFoodAvailabilityProjectionError::PriceFrequencyForbidden),
        code: "price_frequency_forbidden",
    },
    MalformedCase {
        name: "currency_nonletter",
        edits: &[Edit::Replace("price", &["price", "1", "C1D"])],
        expected: domain(FoodAvailabilityError::PriceCurrencyInvalid),
        code: "price_currency_invalid",
    },
    MalformedCase {
        name: "quantity_marker_without_price_unit",
        edits: &[
            Edit::Remove("radroots:price_unit"),
            Edit::Append(&["radroots:quantity", "2", "lb"]),
        ],
        expected: projection(RadrootsFoodAvailabilityProjectionError::PriceUnitMissing),
        code: "price_unit_missing",
    },
    MalformedCase {
        name: "ungoverned_unit",
        edits: &[Edit::Replace(
            "radroots:price_unit",
            &["radroots:price_unit", "crate"],
        )],
        expected: domain(FoodAvailabilityError::PriceUnitInvalid),
        code: "price_unit_invalid",
    },
    MalformedCase {
        name: "one_element_price_unit",
        edits: &[Edit::Replace(
            "radroots:price_unit",
            &["radroots:price_unit"],
        )],
        expected: domain(FoodAvailabilityError::PriceUnitInvalid),
        code: "price_unit_invalid",
    },
    MalformedCase {
        name: "zero_quantity",
        edits: &[Edit::Append(&["radroots:quantity", "000.00", "lb"])],
        expected: domain(FoodAvailabilityError::QuantityZero),
        code: "quantity_zero",
    },
    MalformedCase {
        name: "quantity_unit_mismatch",
        edits: &[Edit::Append(&["radroots:quantity", "2", "kg"])],
        expected: domain(FoodAvailabilityError::QuantityInvalid),
        code: "quantity_invalid",
    },
    MalformedCase {
        name: "quantity_missing_unit",
        edits: &[Edit::Append(&["radroots:quantity", "2"])],
        expected: domain(FoodAvailabilityError::QuantityInvalid),
        code: "quantity_invalid",
    },
    MalformedCase {
        name: "unsupported_status",
        edits: &[Edit::Replace("status", &["status", "withdrawn"])],
        expected: domain(FoodAvailabilityError::StatusInvalid),
        code: "food_status_invalid",
    },
    MalformedCase {
        name: "missing_status",
        edits: &[Edit::Remove("status")],
        expected: projection(RadrootsFoodAvailabilityProjectionError::TagInvalid),
        code: "food_tag_invalid",
    },
];

const fn projection(
    error: RadrootsFoodAvailabilityProjectionError,
) -> RadrootsFoodAvailabilityAdmissionError {
    RadrootsFoodAvailabilityAdmissionError::Projection(error)
}

const fn domain(error: FoodAvailabilityError) -> RadrootsFoodAvailabilityAdmissionError {
    projection(RadrootsFoodAvailabilityProjectionError::Domain(error))
}

pub fn raw_tags(tags: &[&[&str]]) -> Vec<Vec<String>> {
    tags.iter()
        .map(|tag| tag.iter().map(|value| (*value).to_string()).collect())
        .collect()
}

pub fn focused_recipe() -> Recipe {
    Recipe {
        content: CONTENT.into(),
        tags: raw_tags(BASE_TAGS),
    }
}

pub fn recipe_with_tags(tags: &[&[&str]]) -> Recipe {
    Recipe {
        content: CONTENT.into(),
        tags: raw_tags(tags),
    }
}

pub fn edited_recipe(edits: &[Edit]) -> Recipe {
    let mut recipe = focused_recipe();
    for edit in edits {
        match edit {
            Edit::Remove(name) => recipe.tags.retain(|tag| tag[0] != *name),
            Edit::Replace(name, replacement) => {
                let tag = recipe
                    .tags
                    .iter_mut()
                    .find(|tag| tag[0] == *name)
                    .expect("recipe field exists");
                *tag = replacement
                    .iter()
                    .map(|value| (*value).to_string())
                    .collect();
            }
            Edit::Append(tag) => recipe
                .tags
                .push(tag.iter().map(|value| (*value).to_string()).collect()),
            Edit::Content(content) => recipe.content = (*content).into(),
        }
    }
    recipe
}
