use std::fs;
use std::path::PathBuf;

#[path = "src/parser.rs"]
mod parser;

use parser::generate_rust_constants;

const MANIFEST_PATH: &str = "../../../config/product/harvestcircle-v1.properties";
fn main() {
    println!("cargo:rerun-if-changed={MANIFEST_PATH}");
    let source = fs::read_to_string(MANIFEST_PATH).expect("read HarvestCircle product manifest");
    let generated =
        generate_rust_constants(&source).expect("validate HarvestCircle product manifest");

    fs::write(out_file("product_coordinates.rs"), generated)
        .expect("write generated product coordinates");
}

fn out_file(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR")).join(name)
}
