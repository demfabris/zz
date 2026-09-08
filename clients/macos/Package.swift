// swift-tools-version: 6.0

import Foundation
import PackageDescription

let ffiDirectory =
    ProcessInfo.processInfo.environment["ZZ_MACOS_FFI_DIR"]
    ?? URL(fileURLWithPath: #filePath).deletingLastPathComponent()
    .appendingPathComponent("../../target/debug").standardizedFileURL.path

let package = Package(
    name: "ZZMac",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "ZZUI", targets: ["ZZUI"]),
        .executable(name: "ZZComponentGallery", targets: ["ZZComponentGallery"]),
        .executable(name: "ZZNative", targets: ["ZZNative"]),
    ],
    targets: [
        .target(name: "ZZUI"),
        .executableTarget(name: "ZZComponentGallery", dependencies: ["ZZUI"]),
        .systemLibrary(name: "CZZClient"),
        .target(
            name: "ZZNativeCore", dependencies: ["CZZClient"],
            linkerSettings: [
                .unsafeFlags(["-Xlinker", "-load_hidden", "-Xlinker", ffiDirectory + "/libzz_client_ffi.a"]),
                .unsafeFlags(["-Xlinker", "-dead_strip"]),
                .linkedFramework("CoreFoundation"), .linkedFramework("IOKit"),
                .linkedFramework("Metal"), .linkedFramework("IOSurface"), .linkedFramework("QuartzCore"),
                .linkedLibrary("iconv"), .linkedLibrary("c++"),
            ]),
        .executableTarget(name: "ZZNative", dependencies: ["ZZUI", "ZZNativeCore", "CZZClient"]),
        .testTarget(name: "ZZNativeTests", dependencies: ["ZZNativeCore"]),
        .testTarget(name: "ZZUITests", dependencies: ["ZZUI"]),
    ]
)
