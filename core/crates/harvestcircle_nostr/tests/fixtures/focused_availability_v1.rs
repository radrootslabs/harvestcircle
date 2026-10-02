//! Public protocol-conformance inputs. Signing identities exist only during tests.

use radroots_event::food::availability::{
    FoodAvailabilityDetailsParts, FoodAvailabilityStatus, FoodContent, FoodCurrency,
    FoodIdentifier, FoodPrice, FoodPublishedAt, FoodQuantity, FoodText, FoodUnit,
};

pub const CREATED_AT: u64 = 1_800_000_100;

pub struct Input {
    pub identifier: &'static str,
    pub unit: FoodUnit,
    pub price: &'static str,
    pub quantity: Option<&'static str>,
}

pub struct Expected {
    pub identifier: &'static str,
    pub unit: &'static str,
    pub price: &'static str,
    pub quantity: Option<&'static str>,
}

pub const INPUTS: [Input; 10] = [
    Input {
        identifier: "hcav-v1-gram",
        unit: FoodUnit::Gram,
        price: "0.000000000000000000000000001",
        quantity: Some("0.000000000000000000000000002"),
    },
    Input {
        identifier: "hcav-v1-kilogram",
        unit: FoodUnit::Kilogram,
        price: "1234567890123456789012345678",
        quantity: None,
    },
    Input {
        identifier: "hcav-v1-pound",
        unit: FoodUnit::Pound,
        price: "9007199254740993.125",
        quantity: Some("9007199254740993.375"),
    },
    Input {
        identifier: "hcav-v1-ounce",
        unit: FoodUnit::Ounce,
        price: "0",
        quantity: None,
    },
    Input {
        identifier: "hcav-v1-each",
        unit: FoodUnit::Each,
        price: "1",
        quantity: Some("1234567890123456789012345678"),
    },
    Input {
        identifier: "hcav-v1-dozen",
        unit: FoodUnit::Dozen,
        price: "12.25",
        quantity: None,
    },
    Input {
        identifier: "hcav-v1-bunch",
        unit: FoodUnit::Bunch,
        price: "3.75",
        quantity: Some("2.5"),
    },
    Input {
        identifier: "hcav-v1-punnet",
        unit: FoodUnit::Punnet,
        price: "4.125",
        quantity: None,
    },
    Input {
        identifier: "hcav-v1-bag",
        unit: FoodUnit::Bag,
        price: "5.5",
        quantity: Some("17"),
    },
    Input {
        identifier: "hcav-v1-basket",
        unit: FoodUnit::Basket,
        price: "25.01",
        quantity: None,
    },
];

// Expected wire values are recorded independently of the strict typed inputs.
pub const EXPECTED: [Expected; 10] = [
    Expected {
        identifier: "hcav-v1-gram",
        unit: "g",
        price: "0.000000000000000000000000001",
        quantity: Some("0.000000000000000000000000002"),
    },
    Expected {
        identifier: "hcav-v1-kilogram",
        unit: "kg",
        price: "1234567890123456789012345678",
        quantity: None,
    },
    Expected {
        identifier: "hcav-v1-pound",
        unit: "lb",
        price: "9007199254740993.125",
        quantity: Some("9007199254740993.375"),
    },
    Expected {
        identifier: "hcav-v1-ounce",
        unit: "oz",
        price: "0",
        quantity: None,
    },
    Expected {
        identifier: "hcav-v1-each",
        unit: "each",
        price: "1",
        quantity: Some("1234567890123456789012345678"),
    },
    Expected {
        identifier: "hcav-v1-dozen",
        unit: "dozen",
        price: "12.25",
        quantity: None,
    },
    Expected {
        identifier: "hcav-v1-bunch",
        unit: "bunch",
        price: "3.75",
        quantity: Some("2.5"),
    },
    Expected {
        identifier: "hcav-v1-punnet",
        unit: "punnet",
        price: "4.125",
        quantity: None,
    },
    Expected {
        identifier: "hcav-v1-bag",
        unit: "bag",
        price: "5.5",
        quantity: Some("17"),
    },
    Expected {
        identifier: "hcav-v1-basket",
        unit: "basket",
        price: "25.01",
        quantity: None,
    },
];

pub fn details_parts(
    input: &Input,
    status: FoodAvailabilityStatus,
) -> FoodAvailabilityDetailsParts {
    FoodAvailabilityDetailsParts {
        content: FoodContent::new("Public conformance: café, \"exact\" decimals.\nText only.")
            .expect("bounded public content"),
        identifier: FoodIdentifier::parse(input.identifier).expect("bounded identifier"),
        title: FoodText::new("Focused availability conformance").expect("bounded title"),
        summary: FoodText::new("Public protocol fixture").expect("bounded summary"),
        published_at: FoodPublishedAt::new(1_800_000_000).expect("valid publication time"),
        location: FoodText::new("Protocol test location").expect("bounded location"),
        price: FoodPrice::new(
            input.price,
            FoodCurrency::parse("CAD").expect("valid currency"),
            input.unit,
        )
        .expect("exact price"),
        quantity: input
            .quantity
            .map(|amount| FoodQuantity::new(amount, input.unit).expect("positive exact quantity")),
        status,
        images: Vec::new(),
    }
}
