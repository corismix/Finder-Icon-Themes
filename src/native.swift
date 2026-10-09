import AppKit
import Darwin
import Foundation

struct Item: Codable {
    let path: String
    let icon: String?
}
struct Request: Codable {
    let action: String
    let items: [Item]
}
struct Result: Codable {
    let path: String
    let custom: Bool?
    let ok: Bool?
    let error: String?
}

func hasCustomIcon(_ file: String) -> Bool {
    var bytes = [UInt8](repeating: 0, count: 32)
    let count: Int = file.withCString { cFile in
        "com.apple.FinderInfo".withCString { cAttribute in
            bytes.withUnsafeMutableBytes { buffer in
                getxattr(cFile, cAttribute, buffer.baseAddress, buffer.count, 0, 0)
            }
        }
    }
    if count >= 10 {
        let finderFlags = (UInt16(bytes[8]) << 8) | UInt16(bytes[9])
        if (finderFlags & 0x0400) != 0 { return true }
    }
    return FileManager.default.fileExists(atPath: file + "/Icon\r")
}

guard let input = try? FileHandle.standardInput.readToEnd(),
      let request = try? JSONDecoder().decode(Request.self, from: input),
      ["inspect", "set"].contains(request.action) else {
    fputs("Invalid request JSON\n", stderr)
    exit(64)
}

var results: [Result] = []
for item in request.items {
    if request.action == "inspect" {
        results.append(Result(path: item.path, custom: hasCustomIcon(item.path), ok: nil, error: nil))
        continue
    }
    var icon: NSImage? = nil
    if let imagePath = item.icon {
        guard let image = NSImage(contentsOfFile: imagePath) else {
            results.append(Result(path: item.path, custom: nil, ok: false, error: "Cannot load PNG"))
            continue
        }
        icon = image
    }
    let ok = NSWorkspace.shared.setIcon(icon, forFile: item.path, options: [])
    results.append(Result(path: item.path, custom: nil, ok: ok,
                          error: ok ? nil : "NSWorkspace.setIcon failed"))
}
let output = try JSONEncoder().encode(results)
FileHandle.standardOutput.write(output)
