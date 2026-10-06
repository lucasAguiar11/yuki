// Usage: wid <pid>  →  "<windowNumber> <x> <y>" of that process's main window.
// Filtering by PID matters: the user may have their own Yuki open.
import CoreGraphics

let pid = Int(CommandLine.arguments[1])!
let windows = CGWindowListCopyWindowInfo(.optionOnScreenOnly, kCGNullWindowID) as! [[String: Any]]
for w in windows where (w["kCGWindowOwnerPID"] as? Int) == pid && (w["kCGWindowLayer"] as? Int) == 0 {
    let b = w["kCGWindowBounds"] as! [String: Any]
    print(w["kCGWindowNumber"]!, b["X"]!, b["Y"]!)
    break
}
