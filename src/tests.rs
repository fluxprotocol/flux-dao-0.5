#![allow(clippy::needless_pass_by_value)]

use near_sdk::{
    AccountId,
    VMContext,
    testing_env,
    MockedBlockchain,
    json_types::{
        U64,
        U128
    },
    serde_json::json
};

use near_sdk_sim::{
    ExecutionResult,
    transaction::{
        ExecutionOutcome,
        ExecutionStatus
    },
    call,
    deploy,
    init_simulator,
    near_crypto::Signer,
    to_yocto,
    view,
    ContractAccount,
    UserAccount,
    STORAGE_AMOUNT,
    DEFAULT_GAS,
    account::AccessKey
};

const REGISTRY_STORAGE: u128 = 8_300_000_000_000_000_000_000;

/// Load in contract bytes
near_sdk_sim::lazy_static! {
    static ref DAO_WASM_BYTES: &'static [u8] = include_bytes!("../res/flux_dao.wasm").as_ref();
    static ref FLUX_WASM_BYTES: &'static [u8] = include_bytes!("../res/flux_protocol.wasm").as_ref();
}

fn init(

) -> (UserAccount) {
    let master_account = init_simulator(None);
    master_account

}