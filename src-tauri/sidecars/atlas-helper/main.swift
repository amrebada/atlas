// atlas-helper — Swift sidecar for window/screen/active-window enumeration.
//
// Replaces the JXA + System Events path in mcp.rs that broke on macOS 26
// (CGWindowListCopyWindowInfo gated, AX-tree introspection blocked for
// Chrome / VSCode). SCShareableContent is the modern Apple-blessed API
// and only needs Screen Recording — same permission Atlas already holds
// for screenshots.
//
// Subcommands (all emit a single JSON document on stdout, exit 0 on
// success / non-zero with an error message on stderr otherwise):
//
//   list-windows        Every on-screen, non-desktop window.
//   list-screens        Every NSScreen with frame + cgFrame.
//   get-active-window   Frontmost app's frontmost window (or null).
//   version             {"version":"…"} for the Rust shim's probe.
//
// Build: ../../scripts/build-helper.sh

import AppKit
import Foundation
import ScreenCaptureKit

let HELPER_VERSION = "1.0.0"

// MARK: - Output helpers

func writeJSON(_ value: Any) {
    do {
        let data = try JSONSerialization.data(
            withJSONObject: value,
            options: [.fragmentsAllowed]
        )
        FileHandle.standardOutput.write(data)
    } catch {
        die("encode JSON: \(error.localizedDescription)")
    }
}

func die(_ message: String) -> Never {
    if let data = (message + "\n").data(using: .utf8) {
        FileHandle.standardError.write(data)
    }
    exit(1)
}

// MARK: - Subcommands

func listScreens() {
    let screens = NSScreen.screens
    let primaryHeight = screens.first?.frame.size.height ?? 0
    var out: [[String: Any]] = []
    for (i, s) in screens.enumerated() {
        let f = s.frame
        let vf = s.visibleFrame
        let cgY = primaryHeight - (f.origin.y + f.size.height)
        let cgVY = primaryHeight - (vf.origin.y + vf.size.height)
        out.append([
            "index": i,
            "primary": i == 0,
            "frame": ["x": f.origin.x, "y": f.origin.y, "width": f.size.width, "height": f.size.height],
            "visibleFrame": ["x": vf.origin.x, "y": vf.origin.y, "width": vf.size.width, "height": vf.size.height],
            "cgFrame": ["x": f.origin.x, "y": cgY, "width": f.size.width, "height": f.size.height],
            "cgVisibleFrame": ["x": vf.origin.x, "y": cgVY, "width": vf.size.width, "height": vf.size.height],
            "backingScaleFactor": s.backingScaleFactor,
        ])
    }
    writeJSON(out)
}

func windowDict(_ w: SCWindow) -> [String: Any] {
    let app = w.owningApplication
    return [
        "id": w.windowID,
        "owner": app?.applicationName ?? "",
        "bundleId": app?.bundleIdentifier ?? "",
        "pid": app?.processID ?? 0,
        "name": w.title ?? "",
        "bounds": [
            "x": w.frame.origin.x,
            "y": w.frame.origin.y,
            "width": w.frame.size.width,
            "height": w.frame.size.height,
        ],
        "isOnScreen": w.isOnScreen,
        "windowLayer": w.windowLayer,
    ]
}

func listWindows() async {
    do {
        // onScreenWindowsOnly: false — fullscreen apps live in their own
        // Space, so the "on screen right now" filter hides them. Setting
        // this to false also pulls in minimized / hidden windows; we
        // expose `isOnScreen` so callers can tell them apart.
        let content = try await SCShareableContent.excludingDesktopWindows(
            true,
            onScreenWindowsOnly: false
        )
        // Normal app windows live at layer 0. Higher layers are status
        // items, menubars, popovers, the dock — noise for a "give me my
        // open windows" tool. Empty-title windows are usually transient
        // helper windows or detached menus.
        let filtered = content.windows.filter { $0.windowLayer == 0 && !($0.title?.isEmpty ?? true) }
        let out = filtered.map(windowDict)
        writeJSON(out)
    } catch {
        die("SCShareableContent failed: \(error.localizedDescription) — Atlas needs Screen Recording permission (System Settings → Privacy & Security → Screen Recording).")
    }
}

func getActiveWindow() async {
    guard let frontApp = NSWorkspace.shared.frontmostApplication else {
        writeJSON(NSNull())
        return
    }
    let pid = frontApp.processIdentifier
    do {
        let content = try await SCShareableContent.excludingDesktopWindows(
            true,
            onScreenWindowsOnly: false
        )
        // Topmost on-screen window owned by the frontmost app at layer 0.
        // SCWindow's array is in z-order (front to back) per Apple docs;
        // we additionally require isOnScreen so a minimized window of the
        // frontmost app doesn't masquerade as the active one.
        let appWindows = content.windows.filter {
            $0.owningApplication?.processID == pid
                && $0.windowLayer == 0
                && $0.isOnScreen
        }
        if let w = appWindows.first {
            writeJSON(windowDict(w))
        } else {
            writeJSON([
                "owner": frontApp.localizedName ?? "",
                "pid": pid,
                "name": NSNull(),
                "bounds": NSNull(),
            ])
        }
    } catch {
        die("SCShareableContent failed: \(error.localizedDescription)")
    }
}

// MARK: - Entrypoint

let args = CommandLine.arguments
if args.count < 2 {
    die("usage: atlas-helper <list-windows|list-screens|get-active-window|version>")
}

switch args[1] {
case "version":
    writeJSON(["version": HELPER_VERSION])
case "list-screens":
    listScreens()
case "list-windows":
    let sem = DispatchSemaphore(value: 0)
    Task {
        await listWindows()
        sem.signal()
    }
    sem.wait()
case "get-active-window":
    let sem = DispatchSemaphore(value: 0)
    Task {
        await getActiveWindow()
        sem.signal()
    }
    sem.wait()
default:
    die("unknown subcommand: \(args[1])")
}
