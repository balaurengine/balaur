// swift-tools-version:5.9
// The one piece of this engine that has to be Swift: StoreKit 2 has no
// Objective-C interface, so there is nothing for objc2 to bind. It has no
// package dependencies on purpose — the ABI across is a request id and a
// JSON string, so an offline build stays offline.
import PackageDescription

let package = Package(
    name: "balaur-storekit",
    platforms: [.macOS(.v12), .iOS(.v15)],
    products: [
        .library(name: "balaur-storekit", type: .static, targets: ["BalaurStoreKit"])
    ],
    targets: [.target(name: "BalaurStoreKit")]
)
