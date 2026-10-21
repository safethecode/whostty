// Metadata only: no screenshot, window titles or content from other apps.
import AppKit
import CoreGraphics
import Foundation

let pid = Int32(CommandLine.arguments[1])!
let entries = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
let windows = entries.filter {
    ($0[kCGWindowOwnerPID as String] as? Int32) == pid && ($0[kCGWindowLayer as String] as? Int) == 0
}.map { entry -> [String: Any] in
    ["bounds": entry[kCGWindowBounds as String] ?? [:], "on_screen": entry[kCGWindowIsOnscreen as String] ?? false]
}
let output: [String: Any] = [
    "frontmost_pid": NSWorkspace.shared.frontmostApplication?.processIdentifier ?? -1,
    "windows": windows,
    "screens": NSScreen.screens.map { screen -> [String: Any] in
        ["scale": screen.backingScaleFactor, "maximum_fps": screen.maximumFramesPerSecond,
         "width": screen.frame.width, "height": screen.frame.height]
    }
]
let encoded = try JSONSerialization.data(withJSONObject: output, options: [.sortedKeys])
print(String(data: encoded, encoding: .utf8)!)
