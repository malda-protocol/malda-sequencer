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

//! Generated crate containing the image ID and ELF binary of the build guest.
include!(concat!(env!("OUT_DIR"), "/methods.rs"));

#[cfg(test)]
mod tests {

    use alloy_primitives::address;
    use malda_rs::{viewcalls::get_user_balance, constants::*};

    #[tokio::test]
    async fn proves_balance_on_linea() {
        let user = address!("0A047Ec8c33c7E8e9945662F127A5A32c0730190");
        let asset = WETH_LINEA;
        let chain_id = LINEA_CHAIN_ID;

        let session_info =
        get_user_balance(user, asset, chain_id)
            .await
            .unwrap();

        let mcycles_count = session_info
        .segments
        .iter()
        .map(|segment| 1 << segment.po2)
        .sum::<u64>()
        .div_ceil(1_000_000);
        println!("MCycles count: {:?}", mcycles_count);
    }

    #[tokio::test]
    async fn proves_balance_on_optimism() {

        let user = address!("C779b1c9B74948623B6048508aB2F1c9b9370791");
        let asset = WETH_OPTIMISM;
        let chain_id = OPTIMISM_CHAIN_ID;

        let session_info =
        get_user_balance(user, asset, chain_id)
            .await
            .unwrap();

        let mcycles_count = session_info
        .segments
        .iter()
        .map(|segment| 1 << segment.po2)
        .sum::<u64>()
        .div_ceil(1_000_000);
        println!("MCycles count: {:?}", mcycles_count);

    }

    #[tokio::test]
    async fn proves_balance_on_base() {

        let user = address!("C779b1c9B74948623B6048508aB2F1c9b9370791");
        let asset = WETH_BASE;
        let chain_id = BASE_CHAIN_ID;

        let session_info =
        get_user_balance(user, asset, chain_id)
            .await
            .unwrap();

        let mcycles_count = session_info
        .segments
        .iter()
        .map(|segment| 1 << segment.po2)
        .sum::<u64>()
        .div_ceil(1_000_000);
        println!("MCycles count: {:?}", mcycles_count);

    }

    #[tokio::test]
    async fn proves_balance_on_ethereum_via_op() {

        let user = address!("C779b1c9B74948623B6048508aB2F1c9b9370791");
        let asset = WETH_ETHEREUM;
        let chain_id = ETHEREUM_CHAIN_ID;

        let session_info =
        get_user_balance(user, asset, chain_id)
            .await
            .unwrap();

        let mcycles_count = session_info
        .segments
        .iter()
        .map(|segment| 1 << segment.po2)
        .sum::<u64>()
        .div_ceil(1_000_000);
        println!("MCycles count: {:?}", mcycles_count);

    }

}
