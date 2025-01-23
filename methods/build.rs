// Copyright 2023 RISC Zero, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::{collections::HashMap, env, path::PathBuf, fs};

use risc0_build::{embed_methods_with_options, DockerOptions, GuestOptions};
use risc0_build_ethereum::generate_solidity_files;

// Paths where the generated Solidity files will be written.
fn get_project_paths() -> (PathBuf, PathBuf) {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let root_dir = manifest_dir.parent().unwrap().to_path_buf();
    
    let contracts_dir = root_dir.join("contracts");
    let tests_dir = root_dir.join("tests");
    
    // Create directories if they don't exist
    fs::create_dir_all(&contracts_dir).unwrap();
    fs::create_dir_all(&tests_dir).unwrap();
    
    let image_id_path = contracts_dir.join("ImageID.sol");
    let elf_path = tests_dir.join("Elf.sol");
    
    (image_id_path, elf_path)
}

fn main() {
    let (image_id_path, elf_path) = get_project_paths();

    let use_docker = env::var("RISC0_USE_DOCKER").ok().map(|_| DockerOptions {
        root_dir: Some("../".into()),
    });

    // Generate Rust source files for the methods crate.
    let guests = embed_methods_with_options(HashMap::from([(
        "guests",
        GuestOptions {
            features: Vec::new(),
            use_docker,
        },
    )]));

    // Generate Solidity source files for use with Forge.
    let solidity_opts = risc0_build_ethereum::Options::default()
        .with_image_id_sol_path(image_id_path)
        .with_elf_sol_path(elf_path);

    generate_solidity_files(guests.as_slice(), &solidity_opts).unwrap();
}
