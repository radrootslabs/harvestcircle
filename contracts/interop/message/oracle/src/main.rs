// SPDX-License-Identifier: GPL-3.0-only
// Actual immutable public Lib189 Message codec, not an application crypto stack.
#![forbid(unsafe_code)]
use radroots_blossom::Sha256;
use radroots_event::social::message::Message;
use radroots_event_codec::decode::message::message_from_tags;
use radroots_event_codec::encode::message::{message_build_tags, to_wire_parts};
use serde_json::{Value, json};
use std::{collections::BTreeSet, env, fs, path::Path};
const REVISION: &str = "189c49b74b4bafc142b00b76b296477931139e72";
const PROFILE: &str = include_str!("../../source_profile.v1.json");
fn expanded(value: &Value) -> Value {
    match value {
        Value::Object(fields)
            if fields.len() == 2
                && fields.contains_key("repeat")
                && fields.contains_key("count") =>
        {
            let text = fields["repeat"].as_str().unwrap();
            let count = usize::try_from(fields["count"].as_u64().unwrap()).unwrap();
            assert_eq!(text.chars().count(), 1);
            assert!(count <= 8192);
            json!(text.repeat(count))
        }
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, value)| (key.clone(), expanded(value)))
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.iter().map(expanded).collect()),
        _ => value.clone(),
    }
}
fn actual(mode: &str, input: &Value) -> Value {
    if mode == "write" {
        let message: Message = serde_json::from_value(input.clone()).unwrap();
        match to_wire_parts(&message) {
            Ok(parts) => {
                assert_eq!(parts.tags, message_build_tags(&message).unwrap());
                let decoded = message_from_tags(parts.kind, &parts.tags, &parts.content).unwrap();
                json!({"status":"supported","wire_parts":{"kind":parts.kind,"tags":parts.tags,"content":parts.content},"message":decoded})
            }
            Err(error) => json!({"status":"unsupported","error":error.to_string()}),
        }
    } else {
        assert_eq!(mode, "read");
        let kind = u32::try_from(input["kind"].as_u64().unwrap()).unwrap();
        let tags: Vec<Vec<String>> = serde_json::from_value(input["tags"].clone()).unwrap();
        match message_from_tags(kind, &tags, input["content"].as_str().unwrap()) {
            Ok(message) => json!({"status":"supported","message":message}),
            Err(error) => json!({"status":"unsupported","error":error.to_string()}),
        }
    }
}
fn corpus() -> Value {
    let source: Value = serde_json::from_str(PROFILE).unwrap();
    assert_eq!(source["revision"], REVISION);
    let vectors = source["vectors"].as_array().unwrap();
    assert!(vectors.len() > 20 && vectors.len() <= 100);
    let mut ids = BTreeSet::new();
    let rows: Vec<_> = vectors
        .iter()
        .map(|recipe| {
            let mut row = recipe.clone();
            let id = row["id"].as_str().unwrap();
            assert!(ids.insert(id.to_owned()));
            let input = expanded(&row["input"]);
            let result = actual(row["mode"].as_str().unwrap(), &input);
            row["input"] = input;
            row["expected"] = result;
            row
        })
        .collect();
    json!({"revision":REVISION,"source_sha256":Sha256::digest(PROFILE.as_bytes()).to_hex(),"vectors":rows})
}
fn text(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap() + "\n"
}
fn consume(path: &str) -> Value {
    let info = fs::symlink_metadata(path).unwrap();
    assert!(info.is_file() && !info.file_type().is_symlink() && info.len() <= 1024 * 1024);
    let input: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let rows = input.as_array().unwrap();
    assert!(!rows.is_empty() && rows.len() <= 100);
    let mut ids = BTreeSet::new();
    let results: Vec<_> = rows
        .iter()
        .map(|row| {
            let id = row["id"].as_str().unwrap();
            assert!(ids.insert(id.to_owned()));
            let result = actual("read", &row["wire_parts"]);
            assert_eq!(result["status"], "supported");
            assert_eq!(result["message"], row["expected"]);
            json!({"id":id,"message":result["message"]})
        })
        .collect();
    json!({"revision":REVISION,"consumed":results})
}
fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.as_slice() {
        [arg] if arg == "--emit" => print!("{}", text(&corpus())),
        [arg] if arg == "--check" => {
            let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../corpus.v1.json");
            let info = fs::symlink_metadata(&path).unwrap();
            assert!(info.is_file() && !info.file_type().is_symlink() && info.len() <= 1024 * 1024);
            assert_eq!(fs::read_to_string(path).unwrap(), text(&corpus()));
            println!("PASS actual pinned Message corpus freshness");
        }
        [arg, path] if arg == "--consume" => print!("{}", text(&consume(path))),
        _ => panic!("expected --emit, --check or --consume <bounded fixture>"),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_message_vectors_round_trip() {
        let value = corpus();
        let rows = value["vectors"].as_array().unwrap();
        assert!(
            rows.iter()
                .any(|v| v["id"] == "first_enquiry"
                    && v["expected"]["message"]["reply_to"].is_null())
        );
        assert!(
            rows.iter()
                .any(|v| v["expected"]["status"] == "unsupported")
        );
        assert!(rows.iter().any(
            |v| v.get("policy_difference").is_some() && v["expected"]["status"] == "supported"
        ));
    }
    #[test]
    fn native_canonical_tags_are_plain_unsigned_message_parts() {
        let value = corpus();
        let first = value["vectors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["id"] == "first_enquiry")
            .unwrap();
        assert_eq!(first["expected"]["wire_parts"]["kind"], 14);
        assert_eq!(
            first["expected"]["wire_parts"]["tags"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(first["expected"]["wire_parts"].get("sig").is_none());
    }
}
