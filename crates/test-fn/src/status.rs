//! Status-promotion state machine (design §7).

use crate::db::FunctionRow;

pub fn is_cache_hit(row: &FunctionRow) -> bool {
    if row.status != "implemented_verified" {
        return false;
    }
    match (
        &row.signature_hash,
        &row.last_verified_signature_hash,
        &row.implementation_hash,
        &row.last_verified_implementation_hash,
    ) {
        (Some(sig), Some(lv_sig), Some(im), Some(lv_im)) => sig == lv_sig && im == lv_im,
        _ => false,
    }
}
