//! The store's purchases, one set of verbs over StoreKit and Play Billing:
//! `products`, `purchase`, `finish_purchase`, `restore_purchases` and
//! `entitlements`.
//!
//! A store answers in its own words, mapped once into these shapes by its
//! shim (StoreKit's Swift, Play's Java), so a field a store adds reaches a
//! script with nothing here moving. A purchase that lands with nobody
//! asking (another device, a parent approving, a pending payment clearing)
//! is `transaction` on the `platform_event` event. What a purchase proves
//! is the server's to decide, from its `token`: Gamend's hook checks it with
//! the App Store or the Play Developer API, and only then does the game
//! call `finish_purchase`.

use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_core::handler::opt;
use balaur_script::{Module, Value};

use crate::{Call, kind, start_call};

pub(crate) const PURCHASE: &str = "purchase";
pub(crate) const FINISH_PURCHASE: &str = "finish_purchase";
pub(crate) const RESTORE_PURCHASES: &str = "restore_purchases";
const CONSUME: &str = "consume";

pub(crate) fn install(m: &mut Module<Engine>) {
    m.records(&[
        (
            "Product",
            "{ id: string, title: string, description: string, price: float, display_price: string, currency: string, subscription: bool }",
            "One row of a `products` answer's `products`: the store's name and text for it, its price as a number and as the store shows it, the ISO 4217 `currency`, and whether it renews: `let rows: [platform::Product] = reply[\"products\"];`.",
        ),
        (
            "Transaction",
            "{ id: string, product: string, purchased_at: float, quantity: int, revoked: bool, token: string, verified: bool }",
            "A purchase the store holds: a `purchased` answer and a `transaction` arrival carry its fields, and an `entitlements` answer's `items` are each one. `id` is what `finish_purchase` takes. `token` is what a server verifies: the App Store's signed JWS, Play's purchase token. `verified` says the device checked the store's signature, which StoreKit does and Play leaves to a server. `purchased_at` is seconds since 1970.",
        ),
        (
            "FinishPurchaseOptions",
            "{ consume: bool? }",
            "`consume = true` for a product bought again and again: Play consumes it rather than acknowledging it. StoreKit finishes either the same way.",
        ),
    ]);
    m.describe(&[
        (
            kind::PRODUCTS,
            &[],
            "(ids: [string]) -> int",
            "Ask the store about a list of product ids, and return the id whose `products` answer holds what it knows, each a `Product`. An id the store does not know is left out.",
        ),
        (
            PURCHASE,
            &[],
            "(product: string) -> int",
            "Buy a product in the store's own sheet, and return the id its answer carries: `purchased` with the `Transaction`'s fields, `cancelled` or `pending` with `product`, or `error`. A pending one lands later as `transaction` on `platform_event`.",
        ),
        (
            FINISH_PURCHASE,
            &[],
            "(transaction: string, opts: FinishPurchaseOptions?) -> int",
            "Tell the store a purchase is dealt with, once a server has checked its `token`, and return the id its `finished` answer carries. One never finished comes back on every launch, and Play refunds one unacknowledged after three days.",
        ),
        (
            RESTORE_PURCHASES,
            &[],
            "() -> int",
            "Ask the store to hand this player's purchases back, the button a review expects a game to have, and return the id its `restored` answer carries; read them with `entitlements` after.",
        ),
        (
            kind::ENTITLEMENTS,
            &[],
            "() -> int",
            "What this player owns now: return the id whose `entitlements` answer holds them as `items`, each a `Transaction`.",
        ),
    ]);
    m.function(kind::PRODUCTS, |eng: &Engine, ids: Value| {
        let Value::List(ids) = ids else {
            return Err(anyhow!("the product ids should be a list, got {ids:?}"));
        };
        let ids = ids
            .into_iter()
            .map(|id| match id {
                Value::Str(id) => Ok(id),
                other => Err(anyhow!("a product id should be a string, got {other:?}")),
            })
            .collect::<Result<Vec<String>>>()?;
        Ok(start_call(eng, Call::Products { ids }))
    });
    m.function(PURCHASE, |eng: &Engine, product: Value| {
        let product = crate::text(Some(&product), "a product id")?;
        Ok(start_call(eng, Call::Purchase { product }))
    });
    m.function(
        FINISH_PURCHASE,
        |eng: &Engine, (transaction, opts): (Value, Option<Value>)| {
            let transaction = crate::text(Some(&transaction), "a transaction id")?;
            let consume = match opt(opts.as_ref(), CONSUME) {
                Some(Value::Bool(consume)) => *consume,
                Some(other) => {
                    return Err(anyhow!("`consume` should be true or false, got {other:?}"));
                }
                None => false,
            };
            Ok(start_call(
                eng,
                Call::FinishPurchase {
                    transaction,
                    consume,
                },
            ))
        },
    );
    m.function(RESTORE_PURCHASES, |eng: &Engine, ()| {
        Ok(start_call(eng, Call::RestorePurchases))
    });
    m.function(kind::ENTITLEMENTS, |eng: &Engine, ()| {
        Ok(start_call(eng, Call::Entitlements))
    });
}
