// In-app purchases, over the smallest boundary that works: a request id in,
// one JSON object back. Everything StoreKit knows about a product or a
// transaction is in that object, so the shape can grow without the ABI
// moving.
//
// Nothing here decides whether a purchase counts. The signed transaction
// (`jws`) is what a server checks, and a transaction stays unfinished until
// the game says otherwise — which is why `finish` is a call of its own.
import Foundation
import StoreKit

@_silgen_name("balaur_storekit_report")
func balaurReport(_ request: UInt64, _ json: UnsafePointer<CChar>)

private func send(_ request: UInt64, _ payload: [String: Any]) {
    guard let data = try? JSONSerialization.data(withJSONObject: payload),
        let text = String(data: data, encoding: .utf8)
    else {
        return
    }
    text.withCString { balaurReport(request, $0) }
}

private func fail(_ request: UInt64, _ message: String) {
    send(request, ["kind": "failed", "error": message])
}

private func describe(_ product: Product) -> [String: Any] {
    [
        "id": product.id,
        "title": product.displayName,
        "description": product.description,
        "price": (product.price as NSDecimalNumber).doubleValue,
        "display_price": product.displayPrice,
        "kind": String(describing: product.type),
    ]
}

private func describe(_ transaction: Transaction, jws: String, verified: Bool) -> [String: Any] {
    [
        "id": String(transaction.id),
        "product": transaction.productID,
        "purchased_at": transaction.purchaseDate.timeIntervalSince1970,
        "quantity": transaction.purchasedQuantity,
        "revoked": transaction.revocationDate != nil,
        "upgraded": transaction.isUpgraded,
        // What a server verifies. The device's own word is not proof.
        "jws": jws,
        "verified": verified,
    ]
}

private func describe(_ result: VerificationResult<Transaction>) -> [String: Any] {
    switch result {
    case .verified(let transaction):
        return describe(transaction, jws: result.jwsRepresentation, verified: true)
    case .unverified(let transaction, _):
        return describe(transaction, jws: result.jwsRepresentation, verified: false)
    }
}

@_cdecl("balaur_storekit_products")
public func balaurStoreKitProducts(_ request: UInt64, _ ids: UnsafePointer<CChar>) {
    let text = String(cString: ids)
    guard let data = text.data(using: .utf8),
        let wanted = try? JSONSerialization.jsonObject(with: data) as? [String]
    else {
        fail(request, "the product ids were not a list of strings")
        return
    }
    Task {
        do {
            let products = try await Product.products(for: wanted)
            send(request, ["kind": "products", "products": products.map(describe)])
        } catch {
            fail(request, error.localizedDescription)
        }
    }
}

@_cdecl("balaur_storekit_purchase")
public func balaurStoreKitPurchase(_ request: UInt64, _ product: UnsafePointer<CChar>) {
    let id = String(cString: product)
    Task {
        do {
            guard let product = try await Product.products(for: [id]).first else {
                fail(request, "the App Store has no product \(id)")
                return
            }
            switch try await product.purchase() {
            case .success(let result):
                var payload = describe(result)
                payload["kind"] = "purchased"
                send(request, payload)
            case .userCancelled:
                send(request, ["kind": "cancelled", "product": id])
            case .pending:
                send(request, ["kind": "pending", "product": id])
            @unknown default:
                fail(request, "StoreKit answered something this build does not know")
            }
        } catch {
            fail(request, error.localizedDescription)
        }
    }
}

@_cdecl("balaur_storekit_entitlements")
public func balaurStoreKitEntitlements(_ request: UInt64) {
    Task {
        var items: [[String: Any]] = []
        for await entitlement in Transaction.currentEntitlements {
            items.append(describe(entitlement))
        }
        send(request, ["kind": "entitlements", "items": items])
    }
}

@_cdecl("balaur_storekit_restore")
public func balaurStoreKitRestore(_ request: UInt64) {
    Task {
        do {
            try await AppStore.sync()
            send(request, ["kind": "restored"])
        } catch {
            fail(request, error.localizedDescription)
        }
    }
}

@_cdecl("balaur_storekit_finish")
public func balaurStoreKitFinish(_ request: UInt64, _ transaction: UnsafePointer<CChar>) {
    let wanted = String(cString: transaction)
    Task {
        for await result in Transaction.unfinished {
            guard case .verified(let unfinished) = result else { continue }
            if String(unfinished.id) == wanted {
                await unfinished.finish()
                send(request, ["kind": "finished", "id": wanted])
                return
            }
        }
        fail(request, "no unfinished transaction \(wanted)")
    }
}

private var updates: Task<Void, Never>?

/// Transactions that landed somewhere else — another device, a parent
/// approving a request, a subscription renewing. They arrive under request 0,
/// which no call is ever given.
@_cdecl("balaur_storekit_listen")
public func balaurStoreKitListen() {
    guard updates == nil else { return }
    updates = Task.detached {
        for await result in Transaction.updates {
            var payload = describe(result)
            payload["kind"] = "transaction"
            send(0, payload)
        }
    }
}
