use crate::policy_item::{ PolicyItem };
use near_sdk::{ Balance };

pub fn to_yocto(value: u128) -> u128 {
    value * 10_u128.pow(24)
}
